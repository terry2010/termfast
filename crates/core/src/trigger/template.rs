//! Template rendering — FP-4.1
//!
//! Placeholder substitution + conditional blocks.
//! Syntax: {{.VarName}} for placeholders, {{#if VarName}}...{{/if}} for conditionals.
//! Variable values are shell-escaped to prevent command injection.

use std::collections::HashMap;

/// Render a template string with the given variables.
/// Supports:
/// - {{.VarName}} — placeholder substitution (value is shell-escaped)
/// - {{#if VarName}}...{{/if}} — conditional block (renders if VarName is non-empty)
/// - {{#if VarName}}...{{else}}...{{/if}} — conditional with else
pub fn render_template(template: &str, vars: &HashMap<String, String>) -> String {
    // First pass: handle conditionals (which may contain variable substitutions)
    let after_conditionals = render_conditionals(template, vars);
    // Second pass: variable substitution
    render_variables(&after_conditionals, vars)
}

/// Shell-escape a variable value to prevent command injection.
/// Wraps the value in single quotes, escaping any embedded single quotes.
/// This prevents characters like ;, |, &, $, `, etc. from being interpreted by the shell.
fn shell_escape(value: &str) -> String {
    // Replace any single quote with '\'' (end quote, escaped quote, start quote)
    let escaped = value.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

/// Replace {{.VarName}} with shell-escaped values from vars
fn render_variables(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'{') {
            chars.next(); // consume second {
            let tag: String = chars
                .by_ref()
                .take_while(|&c| c != '}')
                .collect();
            // consume the closing }} (both braces)
            while chars.peek() == Some(&'}') {
                chars.next();
            }
            let tag = tag.trim();
            if let Some(var_name) = tag.strip_prefix('.') {
                if let Some(val) = vars.get(var_name) {
                    // Shell-escape the value to prevent injection
                    result.push_str(&shell_escape(val));
                }
            } else {
                // Preserve non-variable tags (shouldn't happen after conditional pass)
                result.push_str("{{");
                result.push_str(tag);
                result.push_str("}}");
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Handle {{#if}}...{{/if}} and {{#if}}...{{else}}...{{/if}} blocks
fn render_conditionals(input: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::new();
    let mut remaining = input;

    while let Some(start) = remaining.find("{{#if ") {
        result.push_str(&remaining[..start]);

        // Find the variable name
        let after_start = &remaining[start + 6..];
        let end_of_tag = after_start.find("}}").unwrap_or(after_start.len());
        let var_name = after_start[..end_of_tag].trim().trim_start_matches('.');

        // Find the matching {{/if}}
        let after_tag = &after_start[end_of_tag + 2..];
        if let Some(end) = find_matching_endif(after_tag) {
            let block_content = &after_tag[..end];
            let after_block = &after_tag[end + 7..]; // skip {{/if}} (7 chars)

            // Check for {{else}}
            if let Some(else_pos) = find_top_level_else(block_content) {
                let if_content = &block_content[..else_pos];
                let else_content = &block_content[else_pos + 8..]; // skip {{else}}

                if is_truthy(var_name, vars) {
                    result.push_str(&render_conditionals(if_content, vars));
                } else {
                    result.push_str(&render_conditionals(else_content, vars));
                }
            } else if is_truthy(var_name, vars) {
                result.push_str(&render_conditionals(block_content, vars));
            }

            remaining = after_block;
        } else {
            // No matching {{/if}}, just output as-is
            result.push_str(&remaining[start..]);
            break;
        }
    }

    result.push_str(remaining);
    result
}

/// Find the matching {{/if}} for a {{#if}}, accounting for nested ifs
fn find_matching_endif(s: &str) -> Option<usize> {
    let mut depth = 1;
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with("{{#if ") {
            depth += 1;
            i += 6;
        } else if s[i..].starts_with("{{/if}}") {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
            i += 7;
        } else {
            i += 1;
        }
    }
    None
}

/// Find a top-level {{else}} (not inside a nested {{#if}})
fn find_top_level_else(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with("{{#if ") {
            depth += 1;
            i += 6;
        } else if s[i..].starts_with("{{/if}}") {
            depth -= 1;
            i += 7;
        } else if depth == 0 && s[i..].starts_with("{{else}}") {
            return Some(i);
        } else {
            i += 1;
        }
    }
    None
}

/// Check if a variable is truthy (non-empty and not "false")
fn is_truthy(var_name: &str, vars: &HashMap<String, String>) -> bool {
    match vars.get(var_name) {
        Some(v) => !v.is_empty() && v.to_lowercase() != "false",
        None => false,
    }
}

/// Compute SHA256 hash of a command list (for template comparison)
pub fn hash_commands(commands: &[String]) -> String {
    use ring::digest::{digest, SHA256};
    let joined = commands.join("\n");
    let hash = digest(&SHA256, joined.as_bytes());
    hex::encode(hash.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_substitution() {
        let mut vars = HashMap::new();
        vars.insert("NewIP".into(), "1.2.3.4".into());
        vars.insert("ProtectedPort".into(), "3000".into());

        let result = render_template("ufw allow from {{.NewIP}} to any port {{.ProtectedPort}}", &vars);
        // Values are now shell-escaped (wrapped in single quotes)
        assert_eq!(result, "ufw allow from '1.2.3.4' to any port '3000'");
    }

    #[test]
    fn test_conditional_block_true() {
        let mut vars = HashMap::new();
        vars.insert("OldIP".into(), "5.6.7.8".into());

        let template = "{{#if OldIP}}remove old {{.OldIP}}{{/if}}";
        let result = render_template(template, &vars);
        assert_eq!(result, "remove old '5.6.7.8'");
    }

    #[test]
    fn test_conditional_block_false() {
        let vars = HashMap::new();
        let template = "{{#if OldIP}}remove old {{.OldIP}}{{/if}}";
        let result = render_template(template, &vars);
        assert_eq!(result, "");
    }

    #[test]
    fn test_conditional_with_else() {
        let vars = HashMap::new();
        let template = "{{#if OldIP}}has old{{else}}no old{{/if}}";
        let result = render_template(template, &vars);
        assert_eq!(result, "no old");
    }

    #[test]
    fn test_firewalld_template() {
        let mut vars = HashMap::new();
        vars.insert("IPFamily".into(), "ipv4".into());
        vars.insert("NewIP".into(), "1.2.3.4".into());
        vars.insert("OldIP".into(), "5.6.7.8".into());
        vars.insert("ProtectedPort".into(), "3000".into());

        let template = "firewall-cmd --permanent --add-rich-rule='rule family=\"{{.IPFamily}}\" source address=\"{{.NewIP}}\" port protocol=\"tcp\" port=\"{{.ProtectedPort}}\" accept'";
        let result = render_template(template, &vars);
        assert!(result.contains("ipv4"));
        assert!(result.contains("1.2.3.4"));
        assert!(result.contains("3000"));
    }

    #[test]
    fn test_firewalld_conditional_remove() {
        let mut vars = HashMap::new();
        vars.insert("IPFamily".into(), "ipv4".into());
        vars.insert("NewIP".into(), "1.2.3.4".into());
        vars.insert("OldIP".into(), "5.6.7.8".into());
        vars.insert("ProtectedPort".into(), "3000".into());

        let template = "{{#if OldIP}}firewall-cmd --permanent --remove-rich-rule='rule family=\"{{.IPFamily}}\" source address=\"{{.OldIP}}\" port protocol=\"tcp\" port=\"{{.ProtectedPort}}\" accept' 2>/dev/null{{/if}}";
        let result = render_template(template, &vars);
        assert!(result.contains("5.6.7.8"));
        assert!(result.contains("remove-rich-rule"));
    }

    #[test]
    fn test_firewalld_conditional_no_oldip() {
        let mut vars = HashMap::new();
        vars.insert("IPFamily".into(), "ipv4".into());
        vars.insert("NewIP".into(), "1.2.3.4".into());
        vars.insert("ProtectedPort".into(), "3000".into());
        // No OldIP

        let template = "{{#if OldIP}}remove old {{.OldIP}}{{/if}}";
        let result = render_template(template, &vars);
        assert_eq!(result, "");
    }

    #[test]
    fn test_nested_conditionals() {
        let mut vars = HashMap::new();
        vars.insert("A".into(), "true".into());
        vars.insert("B".into(), "true".into());

        let template = "{{#if A}}outer{{#if B}}inner{{/if}}{{/if}}";
        let result = render_template(template, &vars);
        assert_eq!(result, "outerinner");
    }

    #[test]
    fn test_nested_conditionals_inner_false() {
        let mut vars = HashMap::new();
        vars.insert("A".into(), "true".into());
        // B not set

        let template = "{{#if A}}outer{{#if B}}inner{{/if}}{{/if}}";
        let result = render_template(template, &vars);
        assert_eq!(result, "outer");
    }

    #[test]
    fn test_missing_variable() {
        let vars = HashMap::new();
        let result = render_template("hello {{.Missing}} world", &vars);
        assert_eq!(result, "hello  world");
    }

    #[test]
    fn test_telegram_template() {
        let mut vars = HashMap::new();
        vars.insert("TelegramToken".into(), "123:ABC".into());
        vars.insert("TelegramChatID".into(), "456".into());
        vars.insert("ServerName".into(), "Tokyo".into());
        vars.insert("NewIP".into(), "1.2.3.4".into());

        let template = "curl --max-time 8 -s 'https://api.telegram.org/bot{{.TelegramToken}}/sendMessage' --data-urlencode 'chat_id={{.TelegramChatID}}' --data-urlencode 'text=VPS {{.ServerName}} reconnected from {{.NewIP}}'";
        let result = render_template(template, &vars);
        assert!(result.contains("123:ABC"));
        assert!(result.contains("456"));
        assert!(result.contains("Tokyo"));
        assert!(result.contains("1.2.3.4"));
    }

    #[test]
    fn test_hash_commands() {
        let cmds = vec!["echo hello".to_string()];
        let hash = hash_commands(&cmds);
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_shell_escape_prevents_injection() {
        let mut vars = HashMap::new();
        // Malicious port value that tries to inject a command
        vars.insert("ProtectedPort".into(), "3000; rm -rf /".into());

        let template = "iptables -A INPUT -p tcp --dport {{.ProtectedPort}} -j DROP";
        let result = render_template(template, &vars);

        // The value should be shell-escaped — wrapped in single quotes
        assert!(result.contains("'3000; rm -rf /'"));
        // The command should NOT contain an unescaped semicolon followed by rm
        assert!(!result.contains("--dport 3000; rm"));
    }

    #[test]
    fn test_shell_escape_single_quote_in_value() {
        let mut vars = HashMap::new();
        vars.insert("Name".into(), "it's a test".into());

        let template = "echo {{.Name}}";
        let result = render_template(template, &vars);

        // Single quotes in the value should be escaped
        assert!(result.contains("it'\\''s a test"));
    }

    #[test]
    fn test_shell_escape_pipe_injection() {
        let mut vars = HashMap::new();
        vars.insert("IP".into(), "1.2.3.4 | cat /etc/passwd".into());

        let template = "firewall-cmd --add-source={{.IP}}";
        let result = render_template(template, &vars);

        // The pipe should be inside single quotes, not interpreted by shell
        assert!(result.contains("'1.2.3.4 | cat /etc/passwd'"));
    }
}
