// Unit tests for oscParser — OSC 9/777/1337 payload parsing
import { describe, it, expect } from "vitest";
import { parseOsc777, parseOsc1337, parseOsc9, parseOsc } from "../oscParser";

describe("parseOsc777", () => {
  it("parses 'Devin finished' as done notification", () => {
    const result = parseOsc777("notify;Devin;Devin finished");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Devin finished",
      done: true,
    });
  });

  it("parses 'Devin needs input' as blocked notification", () => {
    const result = parseOsc777("notify;Devin;Devin needs input");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Devin needs input",
      done: false,
    });
  });

  it("parses 'Tool approval pending' as blocked notification", () => {
    const result = parseOsc777("notify;Devin;Tool approval pending - press q to return");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Tool approval pending - press q to return",
      done: false,
    });
  });

  it("parses 'Question pending' as blocked notification", () => {
    const result = parseOsc777("notify;Devin;Question pending");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Question pending",
      done: false,
    });
  });

  it("parses 'Devin encountered an error' as done notification", () => {
    const result = parseOsc777("notify;Devin;Devin encountered an error");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Devin encountered an error",
      done: true,
    });
  });

  it("parses 'Devin response truncated' as done notification", () => {
    const result = parseOsc777("notify;Devin;Devin response truncated (max tokens)");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Devin response truncated (max tokens)",
      done: true,
    });
  });

  it("parses Devin notify with semicolons in message", () => {
    const result = parseOsc777("notify;Devin;Tool approval pending; press q");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Tool approval pending; press q",
      done: false,
    });
  });

  it("parses Devin notify with empty body (defaults to blocked)", () => {
    const result = parseOsc777("notify;Devin;");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "",
      done: false,
    });
  });

  it("returns null for non-Devin notify", () => {
    expect(parseOsc777("notify;OtherApp;Hello")).toBeNull();
  });

  it("returns null for non-notify command", () => {
    expect(parseOsc777("set-title;Devin;something")).toBeNull();
  });

  it("returns null for too few parts", () => {
    expect(parseOsc777("notify")).toBeNull();
    expect(parseOsc777("notify;Devin")).toBeNull();
  });
});

describe("parseOsc9", () => {
  it("parses 'Devin finished' as done notification", () => {
    const result = parseOsc9("Devin finished");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Devin finished",
      done: true,
    });
  });

  it("parses 'Devin needs input' as blocked notification", () => {
    const result = parseOsc9("Devin needs input");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Devin needs input",
      done: false,
    });
  });

  it("returns null for non-Devin notification", () => {
    expect(parseOsc9("Some other notification")).toBeNull();
  });

  it("returns null for empty payload", () => {
    expect(parseOsc9("")).toBeNull();
    expect(parseOsc9("   ")).toBeNull();
  });
});

describe("parseOsc1337", () => {
  it("parses devin-idle=true as done", () => {
    const result = parseOsc1337("devin-idle=true");
    expect(result).toEqual({ kind: "done", cli: "devin" });
  });

  it("returns null for devin-idle=false (informational only)", () => {
    expect(parseOsc1337("devin-idle=false")).toBeNull();
  });

  it("returns null for unknown 1337 sub-command", () => {
    expect(parseOsc1337("some-other-command")).toBeNull();
  });

  it("trims whitespace", () => {
    const result = parseOsc1337("  devin-idle=true  ");
    expect(result).toEqual({ kind: "done", cli: "devin" });
  });
});

describe("parseOsc dispatcher", () => {
  it("dispatches 777 to parseOsc777 (blocked)", () => {
    const result = parseOsc(777, "notify;Devin;Question pending");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Question pending",
      done: false,
    });
  });

  it("dispatches 777 to parseOsc777 (done)", () => {
    const result = parseOsc(777, "notify;Devin;Devin finished");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Devin finished",
      done: true,
    });
  });

  it("dispatches 9 to parseOsc9 (done)", () => {
    const result = parseOsc(9, "Devin finished");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Devin finished",
      done: true,
    });
  });

  it("dispatches 9 to parseOsc9 (blocked)", () => {
    const result = parseOsc(9, "Devin needs input");
    expect(result).toEqual({
      kind: "notify",
      cli: "devin",
      message: "Devin needs input",
      done: false,
    });
  });

  it("dispatches 1337 to parseOsc1337", () => {
    const result = parseOsc(1337, "devin-idle=true");
    expect(result).toEqual({ kind: "done", cli: "devin" });
  });

  it("returns null for unknown ident", () => {
    expect(parseOsc(999, "whatever")).toBeNull();
  });
});
