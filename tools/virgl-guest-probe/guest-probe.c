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

static void *g_lib = NULL;
static PFN_vkCreateInstance g_CreateInstance = NULL;
static PFN_vkDestroyInstance g_DestroyInstance = NULL;
static PFN_vkGetInstanceProcAddr g_GetInstanceProcAddr = NULL;

static void *load_instance_symbol(const char *name) {
    /* g_GetInstanceProcAddr must be resolved first (via dlsym) to fetch the
     * rest, because the loader can route instance symbols. */
    if (!g_GetInstanceProcAddr)
        return NULL;
    return (void *)g_GetInstanceProcAddr(VK_NULL_HANDLE, name);
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
    g_DestroyInstance = (PFN_vkDestroyInstance)load_instance_symbol("vkDestroyInstance");
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

    PFN_vkEnumeratePhysicalDevices ePhys =
        (PFN_vkEnumeratePhysicalDevices)load_instance_symbol("vkEnumeratePhysicalDevices");
    PFN_vkGetPhysicalDeviceProperties gProps =
        (PFN_vkGetPhysicalDeviceProperties)load_instance_symbol(
            "vkGetPhysicalDeviceProperties");
    PFN_vkGetPhysicalDeviceQueueFamilyProperties gQF =
        (PFN_vkGetPhysicalDeviceQueueFamilyProperties)load_instance_symbol(
            "vkGetPhysicalDeviceQueueFamilyProperties");
    PFN_vkCreateDevice createDev =
        (PFN_vkCreateDevice)load_instance_symbol("vkCreateDevice");

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

    printf("[guest-probe] RESULT: PASS\n");
    return 0;
}