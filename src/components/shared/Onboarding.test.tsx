// Onboarding component tests — FP-8.1
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { Onboarding } from "@/components/shared/Onboarding";

vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn().mockImplementation((cmd: string) => {
    if (cmd === "ipc_add_server") return Promise.resolve("srv_new_1");
    if (cmd === "ipc_check_port_reachable") return Promise.resolve({ reachable: true, latency_ms: 5 });
    if (cmd === "ipc_detect_firewall") return Promise.resolve({ firewall_type: "ufw", listening_ports: [22, 80], firewalld_open_ports: [] });
    if (cmd === "ipc_generate_ssh_key") return Promise.resolve({ key_path: "/tmp/key" });
    if (cmd === "ipc_list_templates") return Promise.resolve([]);
    return Promise.resolve(null);
  }),
}));

describe("Onboarding", () => {
  it("renders without crashing", () => {
    render(<Onboarding onComplete={vi.fn()} />);
    expect(document.body).toBeTruthy();
  });

  it("renders some content on initial render", () => {
    const { container } = render(<Onboarding onComplete={vi.fn()} />);
    expect(container.firstChild).toBeTruthy();
  });
});
