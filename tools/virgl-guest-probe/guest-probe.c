/* guest-probe.c
 *
 * Runs INSIDE a libkrun guest VM.  It dlopens the guest's libvulkan.so.1 (the
 * Vulkan loader), enumerates physical devices through the venus/virtio ICD
 * (VK_DRIVER_FILES -> virtio_icd.x86_64.json), and creates a logical device.
 *
 * This answers the in-guest half of "does virgl venus work": whether the guest
 * virtio-gpu, backed by the host virglrenderer VENUS renderer, exposes a usable
 * Vulkan device.  The host half is answered by tools/virgl-render-server-probe.
 *
 * It is built against vulkan/vulkan.h (for correct struct layouts / ABI) but
 * does NOT link libvulkan -- it dlopens the loader at runtime so it always uses
 * whatever Mesa/Vulkan the guest image ships.
 */
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <vulkan/vulkan.h>

#define VK_LIB "libvulkan.so.1"

typedef VkResult (*PFN_vkCreateInstance)(const VkInstanceCreateInfo *,
                                         const VkAllocationCallbacks *, VkInstance *);
typedef void (*PFN_vkDestroyInstance)(VkInstance, const VkAllocationCallbacks *);
typedef PFN_vkVoidFunction (*PFN_vkGetInstanceProcAddr)(VkInstance, const char *);
typedef VkResult (*PFN_vkEnumeratePhysicalDevices)(VkInstance, uint32_t *,
                                                    VkPhysicalDevice *);
typedef void (*PFN_vkGetPhysicalDeviceProperties)(VkPhysicalDevice,
                                                   VkPhysicalDeviceProperties *);
typedef void (*PFN_vkGetPhysicalDeviceQueueFamilyProperties)(VkPhysicalDevice,
                                                             uint32_t *,
                                                             VkQueueFamilyProperties *);
typedef VkResult (*PFN_vkCreateDevice)(VkPhysicalDevice,
                                       const VkDeviceCreateInfo *,
                                       const VkAllocationCallbacks *, VkDevice *);
typedef void (*PFN_vkDestroyDevice)(VkDevice, const VkAllocationCallbacks *);
typedef void (*PFN_vkGetDeviceQueue)(VkDevice, uint32_t, uint32_t, VkQueue *);
typedef VkResult (*PFN_vkCreateFence)(VkDevice, const VkFenceCreateInfo *,
                                      const VkAllocationCallbacks *, VkFence *);
typedef VkResult (*PFN_vkWaitForFences)(VkDevice, uint32_t, const VkFence *,
                                        VkBool32, uint64_t);
typedef void (*PFN_vkDestroyFence)(VkDevice, VkFence, const VkAllocationCallbacks *);
typedef void (*PFN_vkGetPhysicalDeviceMemoryProperties)(
    VkPhysicalDevice, VkPhysicalDeviceMemoryProperties *);
typedef VkResult (*PFN_vkCreateBuffer)(VkDevice, const VkBufferCreateInfo *,
                                       const VkAllocationCallbacks *, VkBuffer *);
typedef void (*PFN_vkDestroyBuffer)(VkDevice, VkBuffer, const VkAllocationCallbacks *);
typedef void (*PFN_vkGetBufferMemoryRequirements)(VkDevice, VkBuffer,
                                                  VkMemoryRequirements *);
typedef VkResult (*PFN_vkAllocateMemory)(VkDevice, const VkMemoryAllocateInfo *,
                                         const VkAllocationCallbacks *, VkDeviceMemory *);
typedef void (*PFN_vkFreeMemory)(VkDevice, VkDeviceMemory, const VkAllocationCallbacks *);
typedef VkResult (*PFN_vkBindBufferMemory)(VkDevice, VkBuffer, VkDeviceMemory, VkDeviceSize);
typedef VkResult (*PFN_vkMapMemory)(VkDevice, VkDeviceMemory, VkDeviceSize, VkDeviceSize,
                                    VkMemoryMapFlags, void **);
typedef void (*PFN_vkUnmapMemory)(VkDevice, VkDeviceMemory);
typedef VkResult (*PFN_vkCreateCommandPool)(VkDevice, const VkCommandPoolCreateInfo *,
                                            const VkAllocationCallbacks *, VkCommandPool *);
typedef void (*PFN_vkDestroyCommandPool)(VkDevice, VkCommandPool,
                                         const VkAllocationCallbacks *);
typedef VkResult (*PFN_vkAllocateCommandBuffers)(VkDevice,
                                                 const VkCommandBufferAllocateInfo *,
                                                 VkCommandBuffer *);
typedef VkResult (*PFN_vkBeginCommandBuffer)(VkCommandBuffer,
                                             const VkCommandBufferBeginInfo *);
typedef void (*PFN_vkCmdFillBuffer)(VkCommandBuffer, VkBuffer, VkDeviceSize, VkDeviceSize,
                                    uint32_t);
typedef VkResult (*PFN_vkEndCommandBuffer)(VkCommandBuffer);
typedef VkResult (*PFN_vkQueueSubmit)(VkQueue, uint32_t, const VkSubmitInfo *, VkFence);

static void *g_lib = NULL;
static PFN_vkCreateInstance g_CreateInstance = NULL;
static PFN_vkDestroyInstance g_DestroyInstance = NULL;
static PFN_vkGetInstanceProcAddr g_GetInstanceProcAddr = NULL;

/* Fetch a function pointer from the loaded Vulkan loader.
 * For global-level functions (vkGetInstanceProcAddr, vkCreateInstance) use
 * a NULL instance.  For instance-level functions (vkEnumeratePhysicalDevices,
 * vkCreateDevice, etc.) you MUST pass the created VkInstance — the loader
 * does not return instance-level functions from vkGetInstanceProcAddr(NULL). */
static void *get_proc_addr(VkInstance instance, const char *name) {
    if (!g_GetInstanceProcAddr)
        return NULL;
    return (void *)g_GetInstanceProcAddr(instance, name);
}

static int load_loader(void) {
    g_lib = dlopen(VK_LIB, RTLD_NOW | RTLD_LOCAL);
    if (!g_lib) {
        printf("[guest-probe] dlopen(%s) failed: %s\n", VK_LIB, dlerror());
        return 1;
    }
    g_CreateInstance = (PFN_vkCreateInstance)dlsym(g_lib, "vkCreateInstance");
    g_GetInstanceProcAddr =
        (PFN_vkGetInstanceProcAddr)dlsym(g_lib, "vkGetInstanceProcAddr");
    if (!g_CreateInstance || !g_GetInstanceProcAddr) {
        printf("[guest-probe] loader missing vkCreateInstance/vkGetInstanceProcAddr\n");
        return 1;
    }
    return 0;
}

/* chromium-like heavy venus workload: a burst of concurrent submissions plus a
 * sequential multi-submit churn, each submission carrying its own fence.
 * ANGLE/Vulkan init drives many submissions across rings; if the venus ring
 * retirement wedges under load (the chromium exit_code=6 symptom), one of
 * these fences will time out here.  LOW/hangproof: every fence waits with a
 * bounded 10s timeout, then we print which stage (if any) stalled.
 *
 * Returns 0 on success, 1 on a detected submission/fence stall.
 */
#define HEAVY_BURST 8
#define HEAVY_CHURN 16

static int heavy_workload(
    VkDevice device,
    VkQueue queue,
    uint32_t graphics_family,
    void *inst,
    PFN_vkCreateBuffer createBuffer,
    PFN_vkGetBufferMemoryRequirements getReq,
    PFN_vkGetPhysicalDeviceMemoryProperties getMem,
    VkPhysicalDevice phys,
    PFN_vkAllocateMemory alloc,
    PFN_vkBindBufferMemory bind,
    PFN_vkCreateCommandPool createPool,
    PFN_vkAllocateCommandBuffers allocBufs,
    PFN_vkBeginCommandBuffer begin,
    PFN_vkCmdFillBuffer fill,
    PFN_vkEndCommandBuffer end,
    PFN_vkCreateFence createFence,
    PFN_vkWaitForFences wait,
    PFN_vkDestroyFence destroyFence,
    PFN_vkDestroyBuffer destroyBuffer,
    PFN_vkFreeMemory freeMemory,
    PFN_vkDestroyCommandPool destroyPool,
    PFN_vkGetDeviceQueue getQueue,
    PFN_vkQueueSubmit queueSubmit) {
    (void)inst;

    /* New command pool + a second graphics queue exercises a fresh ring. */
    const VkCommandPoolCreateInfo pci = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        .flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
        .queueFamilyIndex = graphics_family,
    };
    VkCommandPool pool = VK_NULL_HANDLE;
    VkResult vr = createPool(device, &pci, NULL, &pool);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] HEAVY: vkCreateCommandPool => %d (FAIL)\n", vr);
        return 1;
    }

    const VkCommandBufferAllocateInfo cbai = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = pool,
        .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        .commandBufferCount = HEAVY_BURST,
    };
    VkCommandBuffer cmds[HEAVY_BURST] = { 0 };
    vr = allocBufs(device, &cbai, cmds);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] HEAVY: vkAllocateCommandBuffers => %d (FAIL)\n", vr);
        destroyPool(device, pool, NULL);
        return 1;
    }

    /* A second ring: use a second queue family if available, else reuse the
     * graphics family (at least exercises a second queue/create path). */
    VkPhysicalDeviceMemoryProperties mem_props;
    getMem(phys, &mem_props);

    uint32_t buf[HEAVY_BURST];
    (void)buf;
    VkBuffer buffers[HEAVY_BURST] = { 0 };
    VkDeviceMemory mems[HEAVY_BURST] = { 0 };
    VkFence fences[HEAVY_BURST] = { 0 };

    for (uint32_t i = 0; i < HEAVY_BURST; i++) {
        const VkBufferCreateInfo bci = {
            .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            .size = 64,
            .usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT,
        };
        vr = createBuffer(device, &bci, NULL, &buffers[i]);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: vkCreateBuffer[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
        VkMemoryRequirements reqs = { 0 };
        getReq(device, buffers[i], &reqs);
        uint32_t mt = UINT32_MAX;
        for (uint32_t j = 0; j < mem_props.memoryTypeCount && j < 32; j++) {
            if ((reqs.memoryTypeBits & (1u << j)) &&
                (mem_props.memoryTypes[j].propertyFlags &
                 VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT) != 0) {
                mt = j;
                break;
            }
        }
        if (mt == UINT32_MAX) {
            printf("[guest-probe] HEAVY: no host-visible mem type (FAIL)\n");
            return 1;
        }
        const VkMemoryAllocateInfo mai = {
            .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            .allocationSize = reqs.size,
            .memoryTypeIndex = mt,
        };
        vr = alloc(device, &mai, NULL, &mems[i]);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: vkAllocateMemory[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
        vr = bind(device, buffers[i], mems[i], 0);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: vkBindBufferMemory[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
        const VkFenceCreateInfo fci = { .sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO };
        vr = createFence(device, &fci, NULL, &fences[i]);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: vkCreateFence[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
    }

    const VkCommandBufferBeginInfo cbbi = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
    };
    for (uint32_t i = 0; i < HEAVY_BURST; i++) {
        vr = begin(cmds[i], &cbbi);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: vkBeginCommandBuffer[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
        fill(cmds[i], buffers[i], 0, VK_WHOLE_SIZE, 0x5a5a5a5au);
        vr = end(cmds[i]);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: vkEndCommandBuffer[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
    }

    /* Burst: submit all 8 concurrently, each with its own fence. */
    VkSubmitInfo sis[HEAVY_BURST];
    for (uint32_t i = 0; i < HEAVY_BURST; i++) {
        sis[i] = (VkSubmitInfo){
            .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO,
            .commandBufferCount = 1,
            .pCommandBuffers = &cmds[i],
        };
    }
    for (uint32_t i = 0; i < HEAVY_BURST; i++) {
        vr = queueSubmit(queue, 1, &sis[i], fences[i]);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: vkQueueSubmit[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
    }
    printf("[guest-probe] HEAVY: submitted %u concurrent bursts\n", HEAVY_BURST);

    /* Wait on all burst fences. */
    const uint64_t timeout_ns = 10ull * 1000ull * 1000ull * 1000ull;
    VkResult w = wait(device, HEAVY_BURST, fences, VK_TRUE, timeout_ns);
    printf("[guest-probe] HEAVY: vkWaitForFences(burst,10s) => %d (%s)\n", w,
           w == VK_SUCCESS    ? "SIGNALED"
           : w == VK_TIMEOUT  ? "TIMEOUT - burst fence never completed"
                              : "ERROR");
    if (w != VK_SUCCESS) {
        printf("[guest-probe] HEAVY RESULT: FAIL - burst stage stalled\n");
        return 1;
    }

    /* Sequential churn: one submit at a time, wait each, HEAVY_CHURN times.
     * Use a fresh fence per step (fences are not auto-reset unless flagged). */
    VkFence churn_fence = VK_NULL_HANDLE;
    const VkFenceCreateInfo fci = { .sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO };
    for (uint32_t i = 0; i < HEAVY_CHURN; i++) {
        VkCommandBuffer c = cmds[i % HEAVY_BURST];
        vr = begin(c, &cbbi);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: churn begin[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
        fill(c, buffers[i % HEAVY_BURST], 0, VK_WHOLE_SIZE, 0x5a5a5a5au);
        vr = end(c);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: churn end[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
        vr = createFence(device, &fci, NULL, &churn_fence);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: churn createFence[%u] => %d (FAIL)\n", i, vr);
            return 1;
        }
        vr = queueSubmit(queue, 1, &sis[i % HEAVY_BURST], churn_fence);
        if (vr != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: churn submit[%u] => %d (FAIL)\n", i, vr);
            destroyFence(device, churn_fence, NULL);
            return 1;
        }
        w = wait(device, 1, &churn_fence, VK_FALSE, timeout_ns);
        if (w != VK_SUCCESS) {
            printf("[guest-probe] HEAVY: churn wait[%u] => %d (%s) (STALL)\n", i, w,
                   w == VK_TIMEOUT ? "TIMEOUT - fence never completed" : "ERROR");
            destroyFence(device, churn_fence, NULL);
            printf("[guest-probe] HEAVY RESULT: FAIL - churn stage stalled at step %u\n", i);
            return 1;
        }
        destroyFence(device, churn_fence, NULL);
    }
    printf("[guest-probe] HEAVY: churn %u submissions completed\n", HEAVY_CHURN);
    printf("[guest-probe] HEAVY RESULT: PASS\n");

    for (uint32_t i = 0; i < HEAVY_BURST; i++) {
        if (fences[i]) destroyFence(device, fences[i], NULL);
        if (buffers[i]) destroyBuffer(device, buffers[i], NULL);
        if (mems[i]) freeMemory(device, mems[i], NULL);
    }
    destroyPool(device, pool, NULL);
    return 0;
}

int main(void) {
    printf("[guest-probe] starting (lib=%s)\n", VK_LIB);
    if (load_loader() != 0)
        return 1;

    VkApplicationInfo app = {
        .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "virgl-guest-probe",
        .applicationVersion = 1,
        .pEngineName = "probe",
        .engineVersion = 1,
        .apiVersion = VK_API_VERSION_1_0,
    };
    VkInstanceCreateInfo ici = {
        .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &app,
    };

    VkInstance instance = VK_NULL_HANDLE;
    VkResult vr = g_CreateInstance(&ici, NULL, &instance);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] vkCreateInstance => %d (FAIL)\n", vr);
        return 1;
    }
    printf("[guest-probe] vkCreateInstance => 0 (instance created)\n");

    /* Instance-level dispatch must be resolved against the created instance
     * (vkGetInstanceProcAddr(NULL, ...) only exposes global-level functions). */
    g_DestroyInstance =
        (PFN_vkDestroyInstance)get_proc_addr(instance, "vkDestroyInstance");
    PFN_vkEnumeratePhysicalDevices ePhys =
        (PFN_vkEnumeratePhysicalDevices)get_proc_addr(instance, "vkEnumeratePhysicalDevices");
    PFN_vkGetPhysicalDeviceProperties gProps =
        (PFN_vkGetPhysicalDeviceProperties)get_proc_addr(
            instance, "vkGetPhysicalDeviceProperties");
    PFN_vkGetPhysicalDeviceQueueFamilyProperties gQF =
        (PFN_vkGetPhysicalDeviceQueueFamilyProperties)get_proc_addr(
            instance, "vkGetPhysicalDeviceQueueFamilyProperties");
    PFN_vkCreateDevice createDev =
        (PFN_vkCreateDevice)get_proc_addr(instance, "vkCreateDevice");

    if (!ePhys || !gProps || !gQF || !createDev) {
        printf("[guest-probe] missing venus/virtio entry points (FAIL)\n");
        return 1;
    }

    uint32_t count = 0;
    vr = ePhys(instance, &count, NULL);
    if (vr != VK_SUCCESS || count == 0) {
        printf("[guest-probe] vkEnumeratePhysicalDevices => %d, count=%u (FAIL: no device)\n",
               vr, count);
        return 1;
    }
    printf("[guest-probe] physical devices = %u\n", count);

    VkPhysicalDevice *devs = calloc(count, sizeof(VkPhysicalDevice));
    if (!devs) {
        printf("[guest-probe] out of memory (FAIL)\n");
        return 1;
    }
    ePhys(instance, &count, devs);

    /* Pick the first device: print its name, find a graphics queue family, and
     * create a logical device from it. */
    VkPhysicalDevice dev = devs[0];
    VkPhysicalDeviceProperties props;
    gProps(dev, &props);
    printf("[guest-probe] device[0] name=\"%s\" vendor=0x%x device=0x%x api=0x%x\n",
           props.deviceName, props.vendorID, props.deviceID, props.apiVersion);

    uint32_t qf_count = 0;
    gQF(dev, &qf_count, NULL);
    VkQueueFamilyProperties *qf = calloc(qf_count, sizeof(VkQueueFamilyProperties));
    if (!qf) {
        printf("[guest-probe] out of memory (FAIL)\n");
        return 1;
    }
    gQF(dev, &qf_count, qf);

    uint32_t graphics_family = UINT32_MAX;
    for (uint32_t i = 0; i < qf_count; i++) {
        if (qf[i].queueFlags & VK_QUEUE_GRAPHICS_BIT) {
            graphics_family = i;
            break;
        }
    }
    if (graphics_family == UINT32_MAX) {
        printf("[guest-probe] no graphics queue family (FAIL)\n");
        return 1;
    }
    printf("[guest-probe] graphics queue family = %u (of %u)\n", graphics_family, qf_count);

    float priority = 1.0f;
    VkDeviceQueueCreateInfo dqci = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex = graphics_family,
        .queueCount = 1,
        .pQueuePriorities = &priority,
    };
    VkDeviceCreateInfo dci = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        .queueCreateInfoCount = 1,
        .pQueueCreateInfos = &dqci,
    };

    VkDevice device = VK_NULL_HANDLE;
    vr = createDev(dev, &dci, NULL, &device);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] vkCreateDevice => %d (FAIL)\n", vr);
        return 1;
    }
    printf("[guest-probe] vkCreateDevice => 0 (logical device created)\n");

    /*
     * Submission is the only Venus fence path not covered by enumeration alone.
     * A fill is a real GPU write, so a signalled fence plus visible data is
     * evidence the EXECBUFFER round-trip and host-side retirement both worked.
     */
    PFN_vkGetDeviceQueue getQueue =
        (PFN_vkGetDeviceQueue)get_proc_addr(instance, "vkGetDeviceQueue");
    PFN_vkCreateFence createFence =
        (PFN_vkCreateFence)get_proc_addr(instance, "vkCreateFence");
    PFN_vkWaitForFences waitForFences =
        (PFN_vkWaitForFences)get_proc_addr(instance, "vkWaitForFences");
    PFN_vkDestroyFence destroyFence =
        (PFN_vkDestroyFence)get_proc_addr(instance, "vkDestroyFence");
    PFN_vkGetPhysicalDeviceMemoryProperties getMemProps =
        (PFN_vkGetPhysicalDeviceMemoryProperties)get_proc_addr(
            instance, "vkGetPhysicalDeviceMemoryProperties");
    PFN_vkCreateBuffer createBuffer =
        (PFN_vkCreateBuffer)get_proc_addr(instance, "vkCreateBuffer");
    PFN_vkDestroyBuffer destroyBuffer =
        (PFN_vkDestroyBuffer)get_proc_addr(instance, "vkDestroyBuffer");
    PFN_vkGetBufferMemoryRequirements getBufReqs =
        (PFN_vkGetBufferMemoryRequirements)get_proc_addr(
            instance, "vkGetBufferMemoryRequirements");
    PFN_vkAllocateMemory allocMemory =
        (PFN_vkAllocateMemory)get_proc_addr(instance, "vkAllocateMemory");
    PFN_vkFreeMemory freeMemory =
        (PFN_vkFreeMemory)get_proc_addr(instance, "vkFreeMemory");
    PFN_vkBindBufferMemory bindBufMem =
        (PFN_vkBindBufferMemory)get_proc_addr(instance, "vkBindBufferMemory");
    PFN_vkMapMemory mapMemory =
        (PFN_vkMapMemory)get_proc_addr(instance, "vkMapMemory");
    PFN_vkUnmapMemory unmapMemory =
        (PFN_vkUnmapMemory)get_proc_addr(instance, "vkUnmapMemory");
    PFN_vkCreateCommandPool createPool =
        (PFN_vkCreateCommandPool)get_proc_addr(instance, "vkCreateCommandPool");
    PFN_vkDestroyCommandPool destroyPool =
        (PFN_vkDestroyCommandPool)get_proc_addr(instance, "vkDestroyCommandPool");
    PFN_vkAllocateCommandBuffers allocCmdBufs =
        (PFN_vkAllocateCommandBuffers)get_proc_addr(instance, "vkAllocateCommandBuffers");
    PFN_vkBeginCommandBuffer beginCmdBuf =
        (PFN_vkBeginCommandBuffer)get_proc_addr(instance, "vkBeginCommandBuffer");
    PFN_vkCmdFillBuffer cmdFill =
        (PFN_vkCmdFillBuffer)get_proc_addr(instance, "vkCmdFillBuffer");
    PFN_vkEndCommandBuffer endCmdBuf =
        (PFN_vkEndCommandBuffer)get_proc_addr(instance, "vkEndCommandBuffer");
    PFN_vkQueueSubmit queueSubmit =
        (PFN_vkQueueSubmit)get_proc_addr(instance, "vkQueueSubmit");

    const char *missing = NULL;
    if (!getQueue) missing = "vkGetDeviceQueue";
    else if (!createFence) missing = "vkCreateFence";
    else if (!waitForFences) missing = "vkWaitForFences";
    else if (!getMemProps) missing = "vkGetPhysicalDeviceMemoryProperties";
    else if (!createBuffer) missing = "vkCreateBuffer";
    else if (!getBufReqs) missing = "vkGetBufferMemoryRequirements";
    else if (!allocMemory) missing = "vkAllocateMemory";
    else if (!bindBufMem) missing = "vkBindBufferMemory";
    else if (!createPool) missing = "vkCreateCommandPool";
    else if (!allocCmdBufs) missing = "vkAllocateCommandBuffers";
    else if (!beginCmdBuf) missing = "vkBeginCommandBuffer";
    else if (!cmdFill) missing = "vkCmdFillBuffer";
    else if (!endCmdBuf) missing = "vkEndCommandBuffer";
    else if (!queueSubmit) missing = "vkQueueSubmit";

    if (missing) {
        printf("[guest-probe] RESULT: INCONCLUSIVE - %s not resolvable\n", missing);
        return 2;
    }

    VkQueue queue = VK_NULL_HANDLE;
    getQueue(device, graphics_family, 0, &queue);

    VkFence fence = VK_NULL_HANDLE;
    const VkFenceCreateInfo fci = { .sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO };
    vr = createFence(device, &fci, NULL, &fence);
    printf("[guest-probe] vkCreateFence => %d\n", vr);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] RESULT: FAIL - vkCreateFence error\n");
        return 1;
    }

    VkPhysicalDeviceMemoryProperties mem_props;
    getMemProps(dev, &mem_props);

    const VkBufferCreateInfo bci = {
        .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        .size = 256,
        .usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT,
    };
    VkBuffer buffer = VK_NULL_HANDLE;
    vr = createBuffer(device, &bci, NULL, &buffer);
    printf("[guest-probe] vkCreateBuffer => %d\n", vr);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] RESULT: FAIL - vkCreateBuffer error\n");
        destroyFence(device, fence, NULL);
        return 1;
    }

    VkMemoryRequirements reqs = { 0 };
    getBufReqs(device, buffer, &reqs);

    uint32_t mem_type = UINT32_MAX;
    for (uint32_t i = 0; i < mem_props.memoryTypeCount && i < 32; i++) {
        if ((reqs.memoryTypeBits & (1u << i)) &&
            (mem_props.memoryTypes[i].propertyFlags &
             VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT) != 0) {
            mem_type = i;
            break;
        }
    }
    printf("[guest-probe] buffer reqs: size=%llu align=%llu typeBits=0x%x "
           "host_visible_type=%u\n",
           (unsigned long long)reqs.size, (unsigned long long)reqs.alignment,
           reqs.memoryTypeBits, mem_type);

    if (mem_type == UINT32_MAX) {
        printf("[guest-probe] RESULT: INCONCLUSIVE - no host-visible memory type\n");
        return 2;
    }

    const VkMemoryAllocateInfo mai = {
        .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .allocationSize = reqs.size,
        .memoryTypeIndex = mem_type,
    };
    VkDeviceMemory memory = VK_NULL_HANDLE;
    vr = allocMemory(device, &mai, NULL, &memory);
    printf("[guest-probe] vkAllocateMemory => %d\n", vr);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] RESULT: FAIL - vkAllocateMemory error\n");
        return 1;
    }

    vr = bindBufMem(device, buffer, memory, 0);
    printf("[guest-probe] vkBindBufferMemory => %d\n", vr);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] RESULT: FAIL - vkBindBufferMemory error\n");
        return 1;
    }

    const VkCommandPoolCreateInfo pci = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        .flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
        .queueFamilyIndex = graphics_family,
    };
    VkCommandPool pool = VK_NULL_HANDLE;
    vr = createPool(device, &pci, NULL, &pool);
    printf("[guest-probe] vkCreateCommandPool => %d\n", vr);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] RESULT: FAIL - vkCreateCommandPool error\n");
        return 1;
    }

    const VkCommandBufferAllocateInfo cbai = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = pool,
        .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        .commandBufferCount = 1,
    };
    VkCommandBuffer cmd = VK_NULL_HANDLE;
    vr = allocCmdBufs(device, &cbai, &cmd);
    printf("[guest-probe] vkAllocateCommandBuffers => %d\n", vr);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] RESULT: FAIL - vkAllocateCommandBuffers error\n");
        return 1;
    }

    const VkCommandBufferBeginInfo cbbi = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
    };
    vr = beginCmdBuf(cmd, &cbbi);
    printf("[guest-probe] vkBeginCommandBuffer => %d\n", vr);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] RESULT: FAIL - vkBeginCommandBuffer error\n");
        return 1;
    }

    cmdFill(cmd, buffer, 0, VK_WHOLE_SIZE, 0x5a5a5a5au);
    vr = endCmdBuf(cmd);
    printf("[guest-probe] vkEndCommandBuffer => %d\n", vr);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] RESULT: FAIL - vkEndCommandBuffer error\n");
        return 1;
    }

    const VkSubmitInfo si = {
        .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO,
        .commandBufferCount = 1,
        .pCommandBuffers = &cmd,
    };
    vr = queueSubmit(queue, 1, &si, fence);
    printf("[guest-probe] vkQueueSubmit => %d\n", vr);
    if (vr != VK_SUCCESS) {
        printf("[guest-probe] RESULT: FAIL - vkQueueSubmit error\n");
        return 1;
    }

    /*
     * The decisive measurement. Venus fences retire on the host when the
     * virtio-gpu worker polls virglrenderer, so a submission whose fence never
     * signals surfaces here as TIMEOUT rather than as an error return.
     */
    const uint64_t timeout_ns = 10ull * 1000ull * 1000ull * 1000ull;
    VkResult wait_res = waitForFences(device, 1, &fence, VK_FALSE, timeout_ns);
    printf("[guest-probe] vkWaitForFences(10s) => %d (%s)\n", wait_res,
           wait_res == VK_SUCCESS    ? "SIGNALED"
           : wait_res == VK_TIMEOUT  ? "TIMEOUT - fence never completed"
                                     : "ERROR");

    int rc = 1;
    if (wait_res == VK_SUCCESS) {
        void *mapped = NULL;
        VkResult mr = mapMemory(device, memory, 0, VK_WHOLE_SIZE, 0, &mapped);
        if (mr == VK_SUCCESS && mapped) {
            const uint32_t *words = (const uint32_t *)mapped;
            int saw_fill = 0;
            for (uint32_t i = 0; i < 64; i++)
                if (words[i] == 0x5a5a5a5au)
                    saw_fill = 1;
            printf("[guest-probe] buffer[0]=0x%08x fill_visible=%d\n", words[0], saw_fill);
            unmapMemory(device, memory);
            if (!saw_fill)
                printf("[guest-probe] NOTE: fence signalled but fill not visible "
                       "(coherency or GPU write issue)\n");
        } else {
            printf("[guest-probe] vkMapMemory => %d (skipping readback)\n", mr);
        }
        printf("[guest-probe] BASELINE: PASS\n");
        rc = 0;
    } else if (wait_res == VK_TIMEOUT) {
        printf("[guest-probe] RESULT: FAIL - EXECBUFFER submitted but fence did not "
               "complete within 10s\n");
    } else {
        printf("[guest-probe] RESULT: FAIL - vkWaitForFences error\n");
    }

    /*
     * Chromium-like heavy stage: burst of concurrent submits + a sequential
     * churn.  If venus ring retirement wedges under load (the exit_code=6
     * symptom), this stage's per-submit fence will time out here, isolating
     * workload shape from loftd's runtime/sandbox context.
     */
    if (rc == 0) {
        int heavy_rc = heavy_workload(
            device, queue, graphics_family, instance,
            createBuffer, getBufReqs, getMemProps, dev,
            allocMemory, bindBufMem, createPool, allocCmdBufs,
            beginCmdBuf, cmdFill, endCmdBuf, createFence,
            waitForFences, destroyFence, destroyBuffer, freeMemory,
            destroyPool, getQueue, queueSubmit);
        if (heavy_rc != 0)
            rc = 1;
    }

    destroyPool(device, pool, NULL);
    destroyBuffer(device, buffer, NULL);
    freeMemory(device, memory, NULL);
    destroyFence(device, fence, NULL);

    PFN_vkDestroyDevice destroyDevice =
        (PFN_vkDestroyDevice)get_proc_addr(instance, "vkDestroyDevice");
    if (destroyDevice)
        destroyDevice(device, NULL);

    return rc;
}