// Shared E2E mock fixtures — provides a realistic Tauri IPC mock
// with full ServerState objects and call tracking.
//
// Tests import `mockTauri()` to install the mock, then use
// `page.evaluate(() => (window as any).__ipcCalls)` to inspect
// which IPC commands were invoked and with what arguments.

import type { Page } from "@playwright/test";

// === SECTION 1 END ===

/** Full ServerState matching the frontend store shape */
export interface MockServer {
  id: string;
  name: string;
  ssh: {
    host: string;
    port: number;
    user: string;
    auth_method: "password" | "key";
    key_path: string;
    key_auto_generated: boolean;
    connection_mode: "single";
    skip_hostkey_verify: boolean;
  };
  proxy: {
    enabled: boolean;
    socks5_port: number;
    http_port: number;
    mixed_port: number;
    max_channels: number;
    channel_idle_timeout: number;
  };
  reconnect: {
    heartbeat_interval: number;
    max_attempts: number;
    initial_backoff_secs: number;
    max_backoff_secs: number;
  };
  ip_check: { enabled: boolean; interval_secs: number };
  last_known_ip: string | null;
  triggers: MockTrigger[];
  suppress_firewall_badge: boolean;
  // Runtime state (added by store after load)
  current_status: "connected" | "connecting" | "reconnecting" | "auth_failed" | "disconnected" | "offline";
  current_ip: string | null;
  connected_since: string | null;
  reconnect_count: number;
  max_attempts: number;
  proxy_running: boolean;
  active_channels: number;
  bytes_in: number;
  bytes_out: number;
}

export interface MockTrigger {
  id: string;
  template_id: string;
  name: string;
  enabled: boolean;
  trigger_type?: string;
  parameters: Record<string, string>;
  commands: string[];
  timeout_secs: number;
  cooldown_secs: number;
  continue_on_error: boolean;
  notify_on_success: boolean;
  notify_on_failure: boolean;
  last_fired_at: string | null;
  template_hash_at_addition: string;
}

export interface MockTemplate {
  id: string;
  name: string;
  trigger_type: string;
  type: string; // alias used by TemplateLibrary component
  description: string;
  built_in: boolean;
  commands: string[];
  timeout_secs: number;
  template_hash: string;
}

export interface MockConfig {
  version: number;
  general: {
    auto_start: boolean;
    minimize_to_tray: boolean;
    theme: "system" | "light" | "dark";
    language: string;
    log_level: string;
    max_log_entries: number;
    log_to_file: boolean;
    log_dir: string;
    log_max_days: number;
    log_max_size_mb: number;
    system_proxy_server_id: string | null;
    proxy_test_url: string;
    crash_reporting: boolean;
    suppress_firewall_badge: boolean;
  };
  trigger_templates: MockTemplate[];
  servers: MockServer[];
}

// === SECTION 2 END ===

/** Default mock servers used across test suites */
export function defaultServers(): MockServer[] {
  return [
    {
      id: "srv_1",
      name: "Tokyo VPS",
      ssh: { host: "1.2.3.4", port: 22, user: "root", auth_method: "password", key_path: "", key_auto_generated: false, connection_mode: "single", skip_hostkey_verify: false },
      proxy: { enabled: true, socks5_port: 1080, http_port: 8080, mixed_port: 0, max_channels: 64, channel_idle_timeout: 300 },
      reconnect: { heartbeat_interval: 15, max_attempts: 10, initial_backoff_secs: 1, max_backoff_secs: 300 },
      ip_check: { enabled: true, interval_secs: 300 },
      last_known_ip: "1.2.3.4",
      triggers: [],
      suppress_firewall_badge: false,
      current_status: "disconnected",
      current_ip: null,
      connected_since: null,
      reconnect_count: 0,
      max_attempts: 10,
      proxy_running: false,
      active_channels: 0, bytes_in: 0, bytes_out: 0,
    },
    {
      id: "srv_2",
      name: "US West",
      ssh: { host: "5.6.7.8", port: 22, user: "admin", auth_method: "key", key_path: "/tmp/key", key_auto_generated: true, connection_mode: "single", skip_hostkey_verify: false },
      proxy: { enabled: false, socks5_port: 1081, http_port: 8081, max_channels: 64, channel_idle_timeout: 300 },
      reconnect: { heartbeat_interval: 15, max_attempts: 10, initial_backoff_secs: 1, max_backoff_secs: 300 },
      ip_check: { enabled: false, interval_secs: 300 },
      last_known_ip: null,
      triggers: [],
      suppress_firewall_badge: false,
      current_status: "disconnected",
      current_ip: null,
      connected_since: null,
      reconnect_count: 0,
      max_attempts: 10,
      proxy_running: false,
      active_channels: 0, bytes_in: 0, bytes_out: 0,
    },
  ];
}

export function defaultTemplates(): MockTemplate[] {
  return [
    {
      id: "tpl_firewalld",
      name: "Firewalld IP Update",
      trigger_type: "OnIpChange",
      type: "OnIpChange",
      description: "Update firewalld whitelist on IP change",
      built_in: true,
      commands: ["firewall-cmd --add-source={{.NewIP}}"],
      timeout_secs: 30,
      template_hash: "abc123",
    },
    {
      id: "tpl_ufw",
      name: "UFW IP Update",
      trigger_type: "OnIpChange",
      type: "OnIpChange",
      description: "Update ufw whitelist on IP change",
      built_in: true,
      commands: ["ufw allow from {{.NewIP}}"],
      timeout_secs: 30,
      template_hash: "def456",
    },
  ];
}

export function defaultConfig(): MockConfig {
  return {
    version: 1,
    general: {
      auto_start: false,
      minimize_to_tray: true,
      theme: "system",
      language: "en",
      log_level: "info",
      max_log_entries: 1000,
      log_to_file: false,
      log_dir: "",
      log_max_days: 30,
      log_max_size_mb: 10,
      system_proxy_server_id: null,
      proxy_test_url: "https://example.com",
      crash_reporting: false,
      suppress_firewall_badge: false,
    },
    trigger_templates: defaultTemplates(),
    servers: defaultServers(),
  };
}

// === SECTION 3 END ===

/**
 * Install a Tauri IPC mock on the page.
 * Records every invoke call into `window.__ipcCalls` (array of {cmd, args, result}).
 * The mock holds a mutable store so that connect/disconnect/toggle actually
 * change the returned state, letting tests assert on real state transitions.
 */
export async function mockTauri(
  page: Page,
  options: {
    servers?: MockServer[];
    config?: MockConfig;
    /** Override specific IPC responses */
    overrides?: Record<string, (args: any) => any>;
    /** Commands that should reject */
    rejectCommands?: Record<string, string>;
  } = {}
): Promise<void> {
  const servers = options.servers ?? defaultServers();
  const config = options.config ?? defaultConfig();

  await page.addInitScript(({ servers, config }) => {
    // Deep clone so the page owns its copy
    const store = {
      servers: JSON.parse(JSON.stringify(servers)) as any[],
      config: JSON.parse(JSON.stringify(config)) as any,
    };
    const calls: { cmd: string; args: any; result: any; error?: string }[] = [];
    (window as any).__ipcCalls = calls;
    (window as any).__mockStore = store;

    function findServer(id: string) {
      return store.servers.find((s) => s.id === id);
    }

    (window as any).__TAURI_INTERNALS__ = {
      invoke: async (cmd: string, args?: any) => {
        // Intercept Tauri event plugin calls — return a fake unlisten ID
        if (cmd === "plugin:event|listen") return `unlisten_${Date.now()}`;
        if (cmd === "plugin:event|unlisten") return null;

        const entry: { cmd: string; args: any; result: any; error?: string } = { cmd, args: args || {}, result: null };
        calls.push(entry);
        try {
          let result: any;
          switch (cmd) {
            case "ipc_get_config":
              result = store.config;
              break;
            case "ipc_list_servers":
              // Return full server objects (the store expects ServerState)
              result = { servers: store.servers };
              break;
            case "ipc_get_daemon_status":
              result = { running: true, server_count: store.servers.length, log_count: 0, version: "0.1.0" };
              break;
            case "ipc_add_server": {
              const cfg = args?.config;
              if (cfg) {
                const newSrv = { ...cfg, current_status: "disconnected", current_ip: null, connected_since: null, reconnect_count: 0, max_attempts: 10, proxy_running: false, active_channels: 0 };
                store.servers.push(newSrv);
                result = newSrv.id;
              } else {
                result = "srv_new";
              }
              break;
            }
            case "ipc_remove_server": {
              const sid = args?.serverId || args?.server_id;
              store.servers = store.servers.filter((s) => s.id !== sid);
              result = null;
              break;
            }
            case "ipc_connect_server": {
              const sid = args?.serverId || args?.server_id;
              const srv = findServer(sid);
              if (srv) {
                srv.current_status = "connected";
                srv.current_ip = srv.last_known_ip || "203.0.113.42";
                srv.connected_since = new Date().toISOString();
              }
              result = { server_id: sid, status: "connected" };
              break;
            }
            case "ipc_disconnect_server": {
              const sid = args?.serverId || args?.server_id;
              const srv = findServer(sid);
              if (srv) {
                srv.current_status = "disconnected";
                srv.current_ip = null;
                srv.connected_since = null;
                srv.proxy_running = false;
              }
              result = { server_id: sid, status: "disconnected" };
              break;
            }
            case "ipc_get_server_status": {
              const sid = args?.serverId || args?.server_id;
              const srv = findServer(sid);
              result = { server_id: sid, status: srv?.current_status || "disconnected", ip: srv?.current_ip || null };
              break;
            }
            case "ipc_toggle_proxy": {
              const sid = args?.serverId || args?.server_id;
              const enabled = args?.enabled;
              const srv = findServer(sid);
              if (srv) srv.proxy_running = enabled;
              result = { server_id: sid, proxy_enabled: enabled };
              break;
            }
            case "ipc_get_proxy_status": {
              const sid = args?.serverId || args?.server_id;
              const srv = findServer(sid);
              result = { server_id: sid, proxy_enabled: srv?.proxy_running || false, socks5_port: srv?.proxy.socks5_port, http_port: srv?.proxy.http_port };
              break;
            }
            case "ipc_set_system_proxy": {
              const sid = args?.serverId || args?.server_id;
              store.config.general.system_proxy_server_id = sid;
              result = { success: true, server_id: sid };
              break;
            }
            case "ipc_clear_system_proxy": {
              store.config.general.system_proxy_server_id = null;
              result = { success: true };
              break;
            }
            case "ipc_get_network_status":
              result = { online: true, interface_count: 1 };
              break;
            case "ipc_get_logs":
              result = { logs: [] };
              break;
            case "ipc_clear_logs":
              result = null;
              break;
            case "ipc_list_templates":
              // Component expects { templates: [...] } shape
              result = { templates: store.config.trigger_templates };
              break;
            case "ipc_list_triggers": {
              const sid = args?.serverId || args?.server_id;
              const srv = findServer(sid);
              result = srv?.triggers || [];
              break;
            }
            case "ipc_add_trigger": {
              const sid = args?.serverId || args?.server_id;
              const srv = findServer(sid);
              if (srv && args?.trigger) {
                srv.triggers.push(args.trigger);
              }
              result = args?.trigger?.id || "trig_new";
              break;
            }
            case "ipc_update_trigger": {
              const sid = args?.server_id;
              const srv = findServer(sid);
              if (srv) {
                const idx = srv.triggers.findIndex((t) => t.id === args?.trigger_id);
                if (idx >= 0) {
                  srv.triggers[idx] = { ...srv.triggers[idx], ...args, id: srv.triggers[idx].id };
                }
              }
              result = { success: true };
              break;
            }
            case "ipc_remove_trigger": {
              const sid = args?.server_id;
              const srv = findServer(sid);
              if (srv) {
                srv.triggers = srv.triggers.filter((t) => t.id !== args?.trigger_id);
              }
              result = null;
              break;
            }
            case "ipc_manual_fire_trigger":
              result = { server_id: args?.serverId, trigger_id: args?.triggerId, status: "fired" };
              break;
            case "ipc_pause_all_triggers":
              result = null;
              break;
            case "ipc_resume_all_triggers":
              result = null;
              break;
            case "ipc_save_credential":
              result = null;
              break;
            case "ipc_switch_auth_method":
              result = { success: true };
              break;
            case "ipc_generate_ssh_key":
              result = { key_path: "/tmp/test_key_ed25519" };
              break;
            case "ipc_check_port_reachable":
              result = { reachable: true, latency_ms: 5 };
              break;
            case "ipc_detect_firewall":
              result = { firewall_type: "ufw", listening_ports: [22, 80, 443, 3000], firewalld_open_ports: [] };
              break;
            case "ipc_export_full":
              result = { blob: "VPG1AAAA", size: 8 };
              break;
            case "ipc_import_full":
              result = { imported: true };
              break;
            case "ipc_set_autostart":
              result = args?.enabled;
              break;
            case "ipc_get_autostart":
              result = false;
              break;
            case "ipc_update_general_config":
              Object.assign(store.config.general, args);
              result = { success: true };
              break;
            case "ipc_update_server": {
              const sid = args?.server_id;
              const srv = findServer(sid);
              if (srv) {
                if (args?.socks5_port != null) srv.proxy.socks5_port = args.socks5_port;
                if (args?.http_port != null) srv.proxy.http_port = args.http_port;
              }
              result = { success: true };
              break;
            }
            case "ipc_add_trigger_from_template": {
              const sid = args?.server_id || args?.serverId;
              const tmplId = args?.template_id || args?.templateId;
              const srv = findServer(sid);
              const tmpl = store.config.trigger_templates.find((t) => t.id === tmplId);
              if (srv && tmpl) {
                const newTrig = {
                  id: `trig_${Date.now()}`,
                  template_id: tmpl.id,
                  name: tmpl.name,
                  enabled: true,
                  parameters: {},
                  commands: tmpl.commands,
                  timeout_secs: tmpl.timeout_secs,
                  cooldown_secs: 0,
                  continue_on_error: false,
                  notify_on_success: false,
                  notify_on_failure: true,
                  last_fired_at: null,
                  template_hash_at_addition: tmpl.template_hash,
                };
                srv.triggers.push(newTrig);
                result = newTrig;
              } else {
                result = null;
              }
              break;
            }
            case "ipc_shutdown":
              result = null;
              break;
            default:
              console.warn(`[mock] unhandled IPC: ${cmd}`, args);
              result = null;
          }
          entry.result = result;
          return result;
        } catch (e) {
          entry.error = String(e);
          throw e;
        }
      },
    };
  }, { servers, config });
}

// === SECTION 4 END ===

/** Helper: get all IPC calls recorded on the page */
export async function getIpcCalls(page: Page): Promise<{ cmd: string; args: any; result: any; error?: string }[]> {
  return page.evaluate(() => (window as any).__ipcCalls || []);
}

/** Helper: get IPC calls filtered by command name */
export async function getCallsFor(page: Page, cmd: string): Promise<{ cmd: string; args: any; result: any }[]> {
  const calls = await getIpcCalls(page);
  return calls.filter((c) => c.cmd === cmd);
}

/** Helper: get the mock store (mutable state) */
export async function getMockStore(page: Page): Promise<{ servers: MockServer[]; config: MockConfig }> {
  return page.evaluate(() => (window as any).__mockStore);
}

/** Helper: wait for the app to finish loading (server list populated) */
export async function waitForAppReady(page: Page, timeout = 10000): Promise<void> {
  await page.goto("/");
  // Wait for either server names or the "Add Server" button to appear
  await page.waitForSelector("text=Tokyo VPS", { timeout }).catch(() => {});
  // Give stores time to hydrate from IPC
  await page.waitForTimeout(500);
}
