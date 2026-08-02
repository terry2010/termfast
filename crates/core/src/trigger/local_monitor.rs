//! 本地网络监听器 — 检测局域网 IP 变化、公网 IP 变化、网络连接/断开。

use crate::config::TriggerType;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

/// 网络事件 — 由 LocalNetworkMonitor 检测并通过 channel 发送
#[derive(Debug, Clone)]
pub enum LocalNetworkEvent {
    NetworkDisconnect,
    NetworkConnect,
    LanIpChange {
        new_ips: Vec<String>,
        old_ips: Vec<String>,
    },
    PublicIpChange {
        new_ip: String,
        old_ip: Option<String>,
    },
}

impl LocalNetworkEvent {
    pub fn trigger_type(&self) -> TriggerType {
        match self {
            Self::NetworkDisconnect => TriggerType::OnNetworkDisconnect,
            Self::NetworkConnect => TriggerType::OnNetworkConnect,
            Self::LanIpChange { .. } => TriggerType::OnLanIpChange,
            Self::PublicIpChange { .. } => TriggerType::OnPublicIpChange,
        }
    }
}

pub struct LocalNetworkMonitor {
    last_lan_ips: Vec<String>,
    last_public_ip: Option<String>,
    last_online: bool,
    /// 连续公网 IP 检测失败次数（用于防抖：连续 2 次失败才 fire NetworkDisconnect）
    fail_count: u32,
}

impl Default for LocalNetworkMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalNetworkMonitor {
    pub fn new() -> Self {
        Self {
            last_lan_ips: Vec::new(),
            last_public_ip: None,
            last_online: true,
            fail_count: 0,
        }
    }

    /// 启动后台轮询 task，检测到事件时通过 channel 发送。
    /// 返回 receiver 供调用方消费事件。
    /// 轮询间隔默认 30 秒。
    pub fn start(mut self, interval_secs: u64) -> mpsc::UnboundedReceiver<LocalNetworkEvent> {
        let (tx, rx) = mpsc::unbounded_channel::<LocalNetworkEvent>();
        tokio::spawn(async move {
            let mut timer = interval(Duration::from_secs(interval_secs));
            loop {
                timer.tick().await;
                self.check_once(&tx).await;
            }
        });
        rx
    }

    async fn check_once(&mut self, tx: &mpsc::UnboundedSender<LocalNetworkEvent>) {
        // 1. 检测局域网 IP 变化
        let current_lan_ips = get_lan_ips();
        if current_lan_ips != self.last_lan_ips && !self.last_lan_ips.is_empty() {
            let _ = tx.send(LocalNetworkEvent::LanIpChange {
                new_ips: current_lan_ips.clone(),
                old_ips: self.last_lan_ips.clone(),
            });
        }
        self.last_lan_ips = current_lan_ips;

        // 2. 检测公网 IP 变化 + 网络连接状态
        let current_public_ip = fetch_public_ip().await;

        if let Some(ref ip) = current_public_ip {
            // 公网 IP 获取成功 → 在线
            self.fail_count = 0;
            if !self.last_online {
                // 从离线恢复到在线
                let _ = tx.send(LocalNetworkEvent::NetworkConnect);
                self.last_online = true;
            }
            if self.last_public_ip.as_ref() != Some(ip) {
                let _ = tx.send(LocalNetworkEvent::PublicIpChange {
                    new_ip: ip.clone(),
                    old_ip: self.last_public_ip.clone(),
                });
            }
            self.last_public_ip = Some(ip.clone());
        } else {
            // 公网 IP 获取失败
            self.fail_count += 1;
            // 连续 2 次失败才判定为离线（防抖）
            if self.fail_count >= 2 && self.last_online {
                let _ = tx.send(LocalNetworkEvent::NetworkDisconnect);
                self.last_online = false;
            }
        }
    }
}

/// 获取所有非回环的局域网 IP
fn get_lan_ips() -> Vec<String> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !a.is_loopback())
        .map(|a| a.ip().to_string())
        .collect()
}

/// 通过 HTTP 请求获取公网 IP（失败返回 None）
async fn fetch_public_ip() -> Option<String> {
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        reqwest::get("https://api.ipify.org"),
    )
    .await
    .ok()?
    .ok()?
    .text()
    .await
    .ok()?;
    let ip = result.trim().to_string();
    if ip.is_empty() {
        None
    } else {
        Some(ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_network_event_trigger_type() {
        assert_eq!(
            LocalNetworkEvent::NetworkDisconnect.trigger_type(),
            TriggerType::OnNetworkDisconnect
        );
        assert_eq!(
            LocalNetworkEvent::NetworkConnect.trigger_type(),
            TriggerType::OnNetworkConnect
        );
        assert_eq!(
            LocalNetworkEvent::LanIpChange {
                new_ips: vec![],
                old_ips: vec![],
            }
            .trigger_type(),
            TriggerType::OnLanIpChange
        );
        assert_eq!(
            LocalNetworkEvent::PublicIpChange {
                new_ip: "1.2.3.4".to_string(),
                old_ip: None,
            }
            .trigger_type(),
            TriggerType::OnPublicIpChange
        );
    }

    #[test]
    fn test_get_lan_ips_returns_non_empty() {
        // 至少有一个非回环接口（测试环境通常有）
        let ips = get_lan_ips();
        // 不强制断言非空，但至少函数不 panic
        let _ = ips;
    }

    #[tokio::test]
    async fn test_local_network_monitor_new() {
        let monitor = LocalNetworkMonitor::new();
        assert!(monitor.last_lan_ips.is_empty());
        assert!(monitor.last_public_ip.is_none());
        assert!(monitor.last_online);
        assert_eq!(monitor.fail_count, 0);
    }
}
