//! What both backends treat as the game's telemetry, for blocking.
//!
//! Both Mahjong Soul and Riichi City report on themselves through Aliyun
//! SLS web tracking (see `crate::inspector::annotate::sls` for what the
//! beacons carry). Every `<project>.<region>.log.aliyuncs.com` host serves
//! nothing but those beacons, so the whole service domain is matched —
//! current and future projects alike. The MITM proxy refuses the `CONNECT`
//! (`proxy::handler`); the chromium backend fails the request inside the
//! browser (`capture::chromium::cdp`).

/// `true` for the Aliyun SLS service domain itself or any host under it.
///
/// Only the domain boundary counts: `log.aliyuncs.com.evil.example.com`
/// and `catalog.aliyuncs.com` are not SLS hosts.
pub fn is_sls_host(host: &str) -> bool {
    host == "log.aliyuncs.com" || host.ends_with(".log.aliyuncs.com")
}

#[cfg(test)]
mod tests {
    use super::is_sls_host;

    #[test]
    fn sls_hosts_are_matched_on_the_domain_boundary() {
        assert!(is_sls_host("log.aliyuncs.com"));
        assert!(is_sls_host("example-client.cn-hongkong.log.aliyuncs.com"));
        assert!(is_sls_host("hwmaj-client.ap-northeast-1.log.aliyuncs.com"));
        assert!(!is_sls_host("catalog.aliyuncs.com"));
        assert!(!is_sls_host("log.aliyuncs.com.evil.example.com"));
        assert!(!is_sls_host("aliyuncs.com"));
        assert!(!is_sls_host("game.maj-soul.com"));
    }
}
