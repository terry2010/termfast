// Unit tests for TmuxSessionPicker — tmux session restore popup
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, waitFor } from "@testing-library/react";
import { TmuxSessionPicker } from "@/components/shared/TmuxSessionPicker";

// Mock react-i18next
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

// Mock @tauri-apps/api/core Channel
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: ((data: ArrayBuffer) => void) | null = null;
    constructor() {
      this.onmessage = null;
    }
  },
}));

// Mock dispatchTerminalOutput
vi.mock("@/components/shared/TerminalView", () => ({
  dispatchTerminalOutput: vi.fn(),
}));

// Mock ipcInvoke
const mockIpcInvoke = vi.fn();
vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: (...args: any[]) => mockIpcInvoke(...args),
}));

const defaultProps = {
  visible: true,
  serverId: "srv_test",
  cols: 80,
  rows: 24,
  openTmuxTabs: {} as Record<string, string>,
  onSessionCreated: vi.fn(),
  onSkipTmux: vi.fn(),
  onSwitchToTab: vi.fn(),
  onCancel: vi.fn(),
};

describe("TmuxSessionPicker", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders nothing when not visible", () => {
    const { container } = render(
      <TmuxSessionPicker {...defaultProps} visible={false} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows loading state initially", async () => {
    mockIpcInvoke.mockReturnValue(new Promise(() => {})); // never resolves
    const { getByText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    expect(getByText("server.tmux_loading")).toBeTruthy();
  });

  it("shows tmux not installed message when tmux_installed=false", async () => {
    mockIpcInvoke.mockResolvedValue({ sessions: [], tmux_installed: false });
    const { getByText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    await waitFor(() => {
      expect(getByText("server.tmux_tmux_not_installed")).toBeTruthy();
    });
  });

  it("shows no sessions message and create button when sessions empty", async () => {
    mockIpcInvoke.mockResolvedValue({ sessions: [], tmux_installed: true });
    const { getByText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    await waitFor(() => {
      expect(getByText("server.tmux_no_sessions")).toBeTruthy();
    });
    expect(getByText("server.tmux_create_new")).toBeTruthy();
  });

  it("renders session list with all 5 fields", async () => {
    const sessions = [
      {
        name: "termfast_abc",
        description: "my dev session",
        created: 1700000000,
        server: "my-server",
        size: [120, 40] as [number, number],
        windows: 3,
        attached_count: 1,
        last_activity: 1700001000,
      },
    ];
    mockIpcInvoke.mockResolvedValue({ sessions, tmux_installed: true });
    const { getByText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    await waitFor(() => {
      expect(getByText("my dev session")).toBeTruthy();
    });
    // Verify all 5 fields are rendered (验收 #3)
    // Fields are rendered as "label: value" so use partial match
    expect(getByText(/server\.tmux_session_created/)).toBeTruthy();
    expect(getByText(/server\.tmux_session_windows/)).toBeTruthy();
    expect(getByText(/server\.tmux_session_attached/)).toBeTruthy();
    expect(getByText(/server\.tmux_session_activity/)).toBeTruthy();
    // Verify field values (windows=3, attached_count=1)
    expect(getByText(/server\.tmux_session_windows.*3/)).toBeTruthy();
    expect(getByText(/server\.tmux_session_attached.*1/)).toBeTruthy();
    expect(getByText("server.tmux_attach")).toBeTruthy();
  });

  it("shows create form when create button is clicked", async () => {
    mockIpcInvoke.mockResolvedValue({ sessions: [], tmux_installed: true });
    const { getByText, getByPlaceholderText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    await waitFor(() => {
      expect(getByText("server.tmux_create_new")).toBeTruthy();
    });
    fireEvent.click(getByText("server.tmux_create_new"));
    await waitFor(() => {
      expect(getByPlaceholderText("server.tmux_description_placeholder")).toBeTruthy();
    });
  });

  it("calls onSessionCreated when attach button is clicked", async () => {
    const sessions = [
      {
        name: "termfast_abc",
        description: "test session",
        created: 1700000000,
        server: "srv",
        size: [80, 24] as [number, number],
        windows: 1,
        attached_count: 0,
        last_activity: 1700001000,
      },
    ];
    mockIpcInvoke
      .mockResolvedValueOnce({ sessions, tmux_installed: true }) // list
      .mockResolvedValueOnce({ session_id: "new_sid", initial_output: "" }); // attach
    const { getByText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    await waitFor(() => {
      expect(getByText("test session")).toBeTruthy();
    });
    fireEvent.click(getByText("server.tmux_attach"));
    await waitFor(() => {
      expect(defaultProps.onSessionCreated).toHaveBeenCalledWith("new_sid", "", "termfast_abc");
    });
  });

  it("hides create form when cancel button is clicked", async () => {
    mockIpcInvoke.mockResolvedValue({ sessions: [], tmux_installed: true });
    const { getByText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    await waitFor(() => {
      expect(getByText("server.tmux_create_new")).toBeTruthy();
    });
    // Click create to show form, then cancel
    fireEvent.click(getByText("server.tmux_create_new"));
    await waitFor(() => {
      expect(getByText("server.tmux_cancel")).toBeTruthy();
    });
    fireEvent.click(getByText("server.tmux_cancel"));
    // Form should hide — create button visible again (no sessions state)
    await waitFor(() => {
      expect(getByText("server.tmux_no_sessions")).toBeTruthy();
    });
  });

  it("calls onSessionCreated when creating a new session with description", async () => {
    mockIpcInvoke
      .mockResolvedValueOnce({ sessions: [], tmux_installed: true }) // list
      .mockResolvedValueOnce({ session_id: "new_sid", initial_output: "hello" }); // create
    const { getByText, getByPlaceholderText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    await waitFor(() => {
      expect(getByText("server.tmux_create_new")).toBeTruthy();
    });
    // Click create to show form
    fireEvent.click(getByText("server.tmux_create_new"));
    await waitFor(() => {
      expect(getByPlaceholderText("server.tmux_description_placeholder")).toBeTruthy();
    });
    // Type description
    const input = getByPlaceholderText("server.tmux_description_placeholder");
    fireEvent.change(input, { target: { value: "my new session" } });
    // Click create button in form
    fireEvent.click(getByText("server.tmux_create_new"));
    await waitFor(() => {
      expect(defaultProps.onSessionCreated).toHaveBeenCalledWith("new_sid", "hello", undefined);
    });
    // Verify ipc_tmux_new_session was called with description
    expect(mockIpcInvoke).toHaveBeenCalledWith(
      "ipc_tmux_new_session",
      expect.objectContaining({ description: "my new session" }),
    );
  });

  it("calls ipc_tmux_list_sessions with correct server_id", async () => {
    mockIpcInvoke.mockResolvedValue({ sessions: [], tmux_installed: true });
    render(<TmuxSessionPicker {...defaultProps} />);
    await waitFor(() => {
      expect(mockIpcInvoke).toHaveBeenCalledWith("ipc_tmux_list_sessions", {
        server_id: "srv_test",
      });
    });
  });

  it("calls onSkipTmux when skip button is clicked (no sessions)", async () => {
    mockIpcInvoke.mockResolvedValue({ sessions: [], tmux_installed: true });
    const { getByText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    await waitFor(() => {
      expect(getByText("server.tmux_no_sessions")).toBeTruthy();
    });
    fireEvent.click(getByText("server.tmux_skip"));
    expect(defaultProps.onSkipTmux).toHaveBeenCalledTimes(1);
  });

  it("calls onSkipTmux when skip button is clicked (with sessions)", async () => {
    const sessions = [
      {
        name: "termfast_abc",
        description: "test",
        created: 1700000000,
        server: "srv",
        size: [80, 24] as [number, number],
        windows: 1,
        attached_count: 0,
        last_activity: 1700001000,
      },
    ];
    mockIpcInvoke.mockResolvedValue({ sessions, tmux_installed: true });
    const { getByText } = render(
      <TmuxSessionPicker {...defaultProps} />,
    );
    await waitFor(() => {
      expect(getByText("test")).toBeTruthy();
    });
    fireEvent.click(getByText("server.tmux_skip"));
    expect(defaultProps.onSkipTmux).toHaveBeenCalledTimes(1);
  });

  it("shows goto-tab button instead of restore for already-open sessions", async () => {
    const sessions = [
      {
        name: "termfast_abc",
        description: "already open session",
        created: 1700000000,
        server: "srv",
        size: [80, 24] as [number, number],
        windows: 1,
        attached_count: 0,
        last_activity: 1700001000,
      },
    ];
    mockIpcInvoke.mockResolvedValue({ sessions, tmux_installed: true });
    const { getByText, queryByText } = render(
      <TmuxSessionPicker
        {...defaultProps}
        openTmuxTabs={{ termfast_abc: "term:sid123" }}
      />,
    );
    await waitFor(() => {
      expect(getByText("already open session")).toBeTruthy();
    });
    // Should show "goto tab" button, not "restore"
    expect(getByText("server.tmux_goto_tab")).toBeTruthy();
    expect(queryByText("server.tmux_attach")).toBeNull();
    // Click should call onSwitchToTab with the tab id
    fireEvent.click(getByText("server.tmux_goto_tab"));
    expect(defaultProps.onSwitchToTab).toHaveBeenCalledWith("term:sid123");
  });
});
