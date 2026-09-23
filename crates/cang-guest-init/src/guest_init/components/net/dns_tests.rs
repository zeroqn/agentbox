use crate::guest_init::components::net::dns::normalize_resolv_conf;

#[test]
fn internal_passt_dns_normalizes_resolv_conf() {
    assert_eq!(
        normalize_resolv_conf(Some(
            "search example.test\nnameserver 8.8.8.8\noptions ndots:1\n"
        )),
        "nameserver 169.254.1.1\nsearch example.test\nnameserver 8.8.8.8\noptions ndots:1\n"
    );
    assert_eq!(
        normalize_resolv_conf(Some("nameserver 8.8.8.8\nnameserver 169.254.1.1\n")),
        "nameserver 169.254.1.1\nnameserver 8.8.8.8\n"
    );
    assert_eq!(normalize_resolv_conf(None), "nameserver 169.254.1.1\n");
}
