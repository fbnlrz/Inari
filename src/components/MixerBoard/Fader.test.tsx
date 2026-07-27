import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, within } from "@testing-library/react";

import { Fader } from "./Fader";

const onChange = vi.fn();
const MAX = 150;

/**
 * Renders a fader and returns its focusable slider. Scoped to this render's
 * container so a test can mount several faders at different values.
 */
const mount = (value: number, max = MAX) => {
  const { container } = render(
    <Fader value={value} max={max} onChange={onChange} label="Game" />,
  );
  return within(container).getByRole("slider");
};

beforeEach(() => {
  onChange.mockReset();
});

describe("slider semantics", () => {
  it("exposes the ARIA state assistive tech reads", () => {
    const slider = mount(75);

    expect(slider).toHaveAttribute("aria-label", "Game");
    expect(slider).toHaveAttribute("aria-orientation", "vertical");
    expect(slider).toHaveAttribute("aria-valuemin", "0");
    expect(slider).toHaveAttribute("aria-valuemax", String(MAX));
    expect(slider).toHaveAttribute("aria-valuenow", "75");
    expect(slider).toHaveAttribute("aria-valuetext", "75%");
  });

  it("is reachable by tab - the strip is otherwise mouse-only", () => {
    expect(mount(75)).toHaveAttribute("tabindex", "0");
  });
});

describe("keyboard interaction", () => {
  it.each([
    ["ArrowUp", 51],
    ["ArrowRight", 51],
    ["ArrowDown", 49],
    ["ArrowLeft", 49],
    ["PageUp", 60],
    ["PageDown", 40],
    ["Home", 0],
    ["End", MAX],
  ])("%s moves the value to %i", (key, expected) => {
    fireEvent.keyDown(mount(50), { key });

    expect(onChange).toHaveBeenCalledWith(expected);
  });

  it("swallows the key so the panel doesn't scroll underneath", () => {
    // fireEvent returns false when the handler called preventDefault.
    expect(fireEvent.keyDown(mount(50), { key: "ArrowUp" })).toBe(false);
  });

  it("ignores keys it doesn't own", () => {
    const slider = mount(50);

    expect(fireEvent.keyDown(slider, { key: "Enter" })).toBe(true);
    expect(fireEvent.keyDown(slider, { key: "a" })).toBe(true);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("clamps at the ends without firing a no-op change", () => {
    fireEvent.keyDown(mount(MAX), { key: "ArrowUp" });
    fireEvent.keyDown(mount(MAX), { key: "End" });
    fireEvent.keyDown(mount(0), { key: "ArrowDown" });
    fireEvent.keyDown(mount(0), { key: "Home" });

    expect(onChange).not.toHaveBeenCalled();
  });

  it("clamps a page jump to the end instead of overshooting", () => {
    fireEvent.keyDown(mount(MAX - 3), { key: "PageUp" });

    expect(onChange).toHaveBeenCalledWith(MAX);
  });
});
