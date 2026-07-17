// useTrayMenu — dynamically build the system tray menu from the frontend
// so it can use i18n and live server data with per-server submenus.
//
// Menu structure:
//   [server1] ►  ├ Connect / Disconnect
//                ├ New Terminal
//                ├ Start Proxy / Stop Proxy
//                ├ Set as System Proxy
//                ├ ──────
//                ├ Copy SOCKS5 / HTTP / Mixed
//                ├ ──────
//                ├ Edit
//                └ Delete
//   [server2] ► ...
//   ──────
//   Settings
//   Show Main Window
//   Quit
//
// If no servers: "Add Server" (clicking shows main window + add dialog)

import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
import { TrayIcon } from "@tauri-apps/api/tray";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ipcInvoke, formatIpcError } from "@/hooks/useIpc";
import { useServerStore } from "@/stores/serverStore";
import type { ServerState } from "@/stores/serverStore";
import i18n from "@/i18n/config";

// Status indicator prefix for server menu items
function statusIndicator(status: string): string {
  switch (status) {
    case "connected": return "🟢";
    case "connecting": return "🟡";
    case "reconnecting": return "🟡";
    case "auth_failed": return "🔴";
    case "offline": return "⚫";
    default: return "⚪";
  }
}

export function useTrayMenu() {
  const { t } = useTranslation();
  const servers = useServerStore((s) => s.servers);
  const selectServer = useServerStore((s) => s.selectServer);
  const rebuildRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    let cancelled = false;
    let currentMenu: Menu | null = null;

    async function buildServerSubmenu(server: ServerState): Promise<Submenu> {
      const isConnected = server.current_status === "connected";
      const proxyEnabled = !!server.proxy_running;
      const socks5Port = server.proxy?.socks5_port || 1080;
      const httpPort = server.proxy?.http_port || 8080;
      const mixedPort = server.proxy?.mixed_port || 0;

      const items: (MenuItem | PredefinedMenuItem)[] = [];

      // Connect / Disconnect
      items.push(
        await MenuItem.new({
          text: isConnected ? t("server.disconnect") : t("server.connect"),
          action: async () => {
            try {
              if (isConnected) {
                await ipcInvoke("ipc_disconnect_server", { serverId: server.id });
              } else {
                await ipcInvoke("ipc_connect_server", { serverId: server.id });
              }
            } catch (err) {
              console.error("tray connect/disconnect failed:", err);
            }
          },
        }),
      );

      // New Terminal
      items.push(
        await MenuItem.new({
          text: t("server.login_server"),
          action: async () => {
            try {
              // Ensure connected first
              if (!isConnected) {
                await ipcInvoke("ipc_connect_server", { serverId: server.id });
              }
              await ipcInvoke("ipc_terminal_open", {
                server_id: server.id,
                cols: 80,
                rows: 24,
              });
              // Show main window so user can see terminal
              const win = getCurrentWindow();
              await win.show();
              await win.setFocus();
              selectServer(server.id);
            } catch (err) {
              console.error("tray open terminal failed:", formatIpcError(err));
            }
          },
        }),
      );

      // Start/Stop Proxy
      items.push(
        await MenuItem.new({
          text: proxyEnabled ? t("server.stop_proxy") : t("server.start_proxy"),
          enabled: isConnected,
          action: async () => {
            try {
              await ipcInvoke("ipc_toggle_proxy", {
                server_id: server.id,
                enabled: !proxyEnabled,
              });
            } catch (err) {
              console.error("tray toggle proxy failed:", err);
            }
          },
        }),
      );

      // Set as System Proxy
      items.push(
        await MenuItem.new({
          text: t("server.set_system_proxy"),
          enabled: isConnected && proxyEnabled,
          action: async () => {
            try {
              await ipcInvoke("ipc_set_system_proxy", { server_id: server.id });
            } catch (err) {
              console.error("tray set system proxy failed:", err);
            }
          },
        }),
      );

      // Separator
      items.push(await PredefinedMenuItem.new({ item: "Separator" }));

      // Copy proxy addresses
      if (mixedPort > 0) {
        items.push(
          await MenuItem.new({
            text: t("server.copy_mixed", { port: mixedPort }),
            action: () => {
              navigator.clipboard.writeText(`127.0.0.1:${mixedPort}`).catch(() => {});
            },
          }),
        );
      } else {
        items.push(
          await MenuItem.new({
            text: t("server.copy_socks5", { port: socks5Port }),
            action: () => {
              navigator.clipboard.writeText(`127.0.0.1:${socks5Port}`).catch(() => {});
            },
          }),
        );
        items.push(
          await MenuItem.new({
            text: t("server.copy_http", { port: httpPort }),
            action: () => {
              navigator.clipboard.writeText(`127.0.0.1:${httpPort}`).catch(() => {});
            },
          }),
        );
      }

      // Separator
      items.push(await PredefinedMenuItem.new({ item: "Separator" }));

      // Edit
      items.push(
        await MenuItem.new({
          text: t("server.edit"),
          action: () => {
            selectServer(server.id);
            window.dispatchEvent(
              new CustomEvent("edit-server", { detail: { serverId: server.id } }),
            );
            // Show main window
            getCurrentWindow().show().then(() => getCurrentWindow().setFocus());
          },
        }),
      );

      // Delete
      items.push(
        await MenuItem.new({
          text: t("server.delete_title"),
          action: () => {
            selectServer(server.id);
            window.dispatchEvent(
              new CustomEvent("delete-server", {
                detail: { serverId: server.id, serverName: server.name },
              }),
            );
            // Show main window so user sees the confirm dialog
            getCurrentWindow().show().then(() => getCurrentWindow().setFocus());
          },
        }),
      );

      const label = `${statusIndicator(server.current_status)} ${server.name}`;
      return Submenu.new({
        text: label,
        items,
      });
    }

    async function rebuild() {
      if (cancelled) return;
      try {
        const tray = await TrayIcon.getById("main-tray");
        if (!tray) return;

        // Build the full menu
        const menuItems: (MenuItem | PredefinedMenuItem | Submenu)[] = [];

        if (servers.length === 0) {
          // No servers — show "Add Server"
          menuItems.push(
            await MenuItem.new({
              text: t("menu.add_server"),
              action: () => {
                window.dispatchEvent(new CustomEvent("tray-add-server"));
                getCurrentWindow().show().then(() => getCurrentWindow().setFocus());
              },
            }),
          );
        } else {
          // Server list with submenus
          // Sort: abnormal first
          const abnormal = ["auth_failed", "reconnecting", "offline"];
          const sorted = [...servers].sort((a, b) => {
            const aAb = abnormal.includes(a.current_status) ? 0 : 1;
            const bAb = abnormal.includes(b.current_status) ? 0 : 1;
            return aAb - bAb;
          });

          for (const server of sorted) {
            menuItems.push(await buildServerSubmenu(server));
          }
        }

        // Separator
        menuItems.push(await PredefinedMenuItem.new({ item: "Separator" }));

        // Settings
        menuItems.push(
          await MenuItem.new({
            text: t("menu.settings"),
            action: () => {
              window.dispatchEvent(new CustomEvent("tray-open-settings"));
              getCurrentWindow().show().then(() => getCurrentWindow().setFocus());
            },
          }),
        );

        // Show Main Window
        menuItems.push(
          await MenuItem.new({
            text: t("menu.show_window"),
            action: () => {
              getCurrentWindow().show().then(() => getCurrentWindow().setFocus());
            },
          }),
        );

        // Quit
        menuItems.push(
          await MenuItem.new({
            text: t("menu.quit"),
            action: () => {
              // Graceful shutdown via IPC (bypasses minimize_to_tray)
              ipcInvoke("ipc_quit_app", {}).catch(() => {});
            },
          }),
        );

        const newMenu = await Menu.new({ items: menuItems });
        await tray.setMenu(newMenu);

        // Free old menu resources
        if (currentMenu) {
          // Tauri handles cleanup internally; we just drop the reference
        }
        currentMenu = newMenu;
      } catch (err) {
        console.error("[useTrayMenu] failed to rebuild tray menu:", err);
      }
    }

    rebuildRef.current = rebuild;
    rebuild();

    return () => {
      cancelled = true;
    };
  }, [servers, t, selectServer]);

  // Rebuild when language changes
  useEffect(() => {
    const handler = () => {
      rebuildRef.current?.();
    };
    i18n.on("languageChanged", handler);
    return () => {
      i18n.off("languageChanged", handler);
    };
  }, []);
}
