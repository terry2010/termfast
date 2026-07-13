// TitleBar component tests
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { TitleBar } from "@/components/desktop/TitleBar";

describe("TitleBar", () => {
  it("renders title bar with app name", () => {
    render(<TitleBar />);
    expect(screen.getByText(/VPS Guard/i)).toBeInTheDocument();
  });
});
