//! Built-in trigger templates — FP-1.2 / §14
//!
//! 5 built-in templates: firewalld, ufw, process restart, Telegram notify, port alive check.

use super::config::{ParameterSchema, TriggerTemplate, TriggerType};

/// Return all 5 built-in templates (§14.1-14.5)
pub fn all_builtin_templates() -> Vec<TriggerTemplate> {
    vec![
        firewalld_template(),
        ufw_template(),
        process_restart_template(),
        telegram_notify_template(),
        port_alive_template(),
    ]
}

/// §14.1 — Update firewalld whitelist on IP change
fn firewalld_template() -> TriggerTemplate {
    TriggerTemplate {
        id: "tpl_firewalld".into(),
        name: "更新防火墙白名单（firewalld）".into(),
        trigger_type: TriggerType::OnIpChange,
        description: "IP 变化时更新 firewalld 白名单规则".into(),
        built_in: true,
        template_version: 1,
        parameters_schema: vec![ParameterSchema {
            name: "ProtectedPort".into(),
            label: "受保护服务端口".into(),
            param_type: "port".into(),
            required: true,
            default: "3000".into(),
            validation: "1-65535".into(),
        }],
        commands: vec![
            "firewall-cmd --permanent --add-rich-rule='rule family=\"{{.IPFamily}}\" source address=\"{{.NewIP}}\" port protocol=\"tcp\" port=\"{{.ProtectedPort}}\" accept'".into(),
            "{{#if OldIP}}firewall-cmd --permanent --remove-rich-rule='rule family=\"{{.IPFamily}}\" source address=\"{{.OldIP}}\" port protocol=\"tcp\" port=\"{{.ProtectedPort}}\" accept' 2>/dev/null{{/if}}".into(),
            "firewall-cmd --reload".into(),
        ],
        check_target: String::new(),
        check_interval: 60,
        timeout_secs: 30,
    }
}

/// §14.2 — Update ufw whitelist on IP change
fn ufw_template() -> TriggerTemplate {
    TriggerTemplate {
        id: "tpl_ufw".into(),
        name: "更新防火墙白名单（ufw）".into(),
        trigger_type: TriggerType::OnIpChange,
        description: "IP 变化时更新 ufw 白名单规则".into(),
        built_in: true,
        template_version: 1,
        parameters_schema: vec![ParameterSchema {
            name: "ProtectedPort".into(),
            label: "受保护服务端口".into(),
            param_type: "port".into(),
            required: true,
            default: "3000".into(),
            validation: "1-65535".into(),
        }],
        commands: vec![
            "ufw allow from {{.NewIP}} to any port {{.ProtectedPort}}".into(),
            "{{#if OldIP}}ufw delete allow from {{.OldIP}} to any port {{.ProtectedPort}} 2>/dev/null{{/if}}".into(),
        ],
        check_target: String::new(),
        check_interval: 60,
        timeout_secs: 30,
    }
}

/// §14.3 — Restart process when dead
fn process_restart_template() -> TriggerTemplate {
    TriggerTemplate {
        id: "tpl_process_restart".into(),
        name: "重启进程".into(),
        trigger_type: TriggerType::OnProcessDead,
        description: "进程不存在时自动重启".into(),
        built_in: true,
        template_version: 1,
        parameters_schema: vec![ParameterSchema {
            name: "ProcessName".into(),
            label: "进程名".into(),
            param_type: "string".into(),
            required: true,
            default: "nginx".into(),
            validation: String::new(),
        }],
        commands: vec!["systemctl restart {{.ProcessName}}".into()],
        check_target: "nginx".into(),
        check_interval: 60,
        timeout_secs: 30,
    }
}

/// §14.4 — Telegram notification on reconnect
fn telegram_notify_template() -> TriggerTemplate {
    TriggerTemplate {
        id: "tpl_telegram_notify".into(),
        name: "重连通知（Telegram）".into(),
        trigger_type: TriggerType::OnReconnect,
        description: "重连后发送 Telegram 通知".into(),
        built_in: true,
        template_version: 1,
        parameters_schema: vec![
            ParameterSchema {
                name: "TelegramToken".into(),
                label: "Bot Token".into(),
                param_type: "token".into(),
                required: true,
                default: String::new(),
                validation: String::new(),
            },
            ParameterSchema {
                name: "TelegramChatID".into(),
                label: "Chat ID".into(),
                param_type: "string".into(),
                required: true,
                default: String::new(),
                validation: String::new(),
            },
        ],
        commands: vec![
            "curl --max-time 8 -s 'https://api.telegram.org/bot{{.TelegramToken}}/sendMessage' --data-urlencode 'chat_id={{.TelegramChatID}}' --data-urlencode 'text=VPS {{.ServerName}} reconnected from {{.NewIP}}'".into(),
        ],
        check_target: String::new(),
        check_interval: 60,
        timeout_secs: 10,
    }
}

/// §14.5 — Port alive check
fn port_alive_template() -> TriggerTemplate {
    TriggerTemplate {
        id: "tpl_port_alive".into(),
        name: "端口存活检查".into(),
        trigger_type: TriggerType::OnPortClosed,
        description: "端口关闭时自动重启服务".into(),
        built_in: true,
        template_version: 1,
        parameters_schema: vec![
            ParameterSchema {
                name: "CheckPort".into(),
                label: "检查端口".into(),
                param_type: "port".into(),
                required: true,
                default: "80".into(),
                validation: "1-65535".into(),
            },
            ParameterSchema {
                name: "ServiceName".into(),
                label: "服务名".into(),
                param_type: "string".into(),
                required: true,
                default: "nginx".into(),
                validation: String::new(),
            },
        ],
        commands: vec!["systemctl restart {{.ServiceName}}".into()],
        check_target: "80".into(),
        check_interval: 60,
        timeout_secs: 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_five_builtin_templates() {
        let templates = all_builtin_templates();
        assert_eq!(
            templates.len(),
            5,
            "should have exactly 5 built-in templates"
        );
        assert!(templates.iter().all(|t| t.built_in));
    }

    #[test]
    fn test_template_ids_unique() {
        let templates = all_builtin_templates();
        let ids: Vec<_> = templates.iter().map(|t| t.id.as_str()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "template IDs should be unique");
    }

    #[test]
    fn test_firewalld_template() {
        let t = firewalld_template();
        assert_eq!(t.id, "tpl_firewalld");
        assert_eq!(t.trigger_type, TriggerType::OnIpChange);
        assert_eq!(t.commands.len(), 3);
        assert!(t.commands[0].contains("firewall-cmd"));
    }

    #[test]
    fn test_ufw_template() {
        let t = ufw_template();
        assert_eq!(t.id, "tpl_ufw");
        assert_eq!(t.trigger_type, TriggerType::OnIpChange);
        assert_eq!(t.commands.len(), 2);
        assert!(t.commands[0].contains("ufw"));
    }

    #[test]
    fn test_process_restart_template() {
        let t = process_restart_template();
        assert_eq!(t.id, "tpl_process_restart");
        assert_eq!(t.trigger_type, TriggerType::OnProcessDead);
        assert_eq!(t.commands.len(), 1);
        assert!(t.commands[0].contains("systemctl restart"));
    }

    #[test]
    fn test_telegram_template() {
        let t = telegram_notify_template();
        assert_eq!(t.id, "tpl_telegram_notify");
        assert_eq!(t.trigger_type, TriggerType::OnReconnect);
        assert_eq!(t.timeout_secs, 10);
        assert!(t.commands[0].contains("telegram.org"));
    }

    #[test]
    fn test_port_alive_template() {
        let t = port_alive_template();
        assert_eq!(t.id, "tpl_port_alive");
        assert_eq!(t.trigger_type, TriggerType::OnPortClosed);
    }
}
