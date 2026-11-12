import { render, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { webUrl } from "./api";
import { Linkify, TaskLinks, TaskWhy } from "./links";

// Issue #143: only a full http(s) address becomes a link, it opens in a new tab, and text is never HTML.
// Each test queries its own render: this suite keeps earlier renders mounted.
describe("task links", () => {
  it("accepts only full http(s) addresses", () => {
    expect(webUrl("https://jira.example.com/browse/SHOP-12")).toBe("https://jira.example.com/browse/SHOP-12");
    expect(webUrl("HTTP://x.example.com")).toBe("http://x.example.com/");
    for (const bad of ["javascript:alert(1)", " javascript:alert(1)", "JaVaScRiPt:alert(1)", "data:text/html,<b>x</b>", "file:///etc/hosts", "https://", "/relative/path", "#checkout", "src/app.py", "vbscript:x"]) {
      expect(webUrl(bad), bad).toBeNull();
    }
  });

  it("shows each link as an anchor that opens in a new tab, and any other value as text", () => {
    const { container } = render(<TaskLinks links={[
      { label: "ticket", url: "https://jira.example.com/browse/SHOP-12" },
      { label: "github.example.com/…/app.py", url: "https://github.example.com/acme/shop/blob/main/src/app.py" },
      { label: "javascript:alert(1)", url: "javascript:alert(1)" },
      { label: "post in", url: "#checkout" },
    ]} />);
    const view = within(container);
    const ticket = view.getByRole("link", { name: "ticket" });
    expect(ticket).toHaveAttribute("href", "https://jira.example.com/browse/SHOP-12");
    expect(ticket).toHaveAttribute("target", "_blank");
    expect(ticket).toHaveAttribute("rel", "noreferrer noopener");
    expect(view.getByRole("link", { name: "github.example.com/…/app.py" })).toHaveAttribute("target", "_blank");
    expect(view.getAllByRole("link")).toHaveLength(2);
    expect(view.getByText("javascript:alert(1)").closest("a")).toBeNull();
    expect(view.getByText("post in: #checkout").closest("a")).toBeNull();
  });

  it("renders nothing for a task with no links or no why", () => {
    const { container } = render(<><TaskLinks links={[]} /><TaskLinks /><TaskWhy text="" /><TaskWhy /></>);
    expect(container).toBeEmptyDOMElement();
  });

  it("turns the URLs in the why into links and keeps the rest as text, HTML included", () => {
    const text = 'see https://ci.example.com/builds/812, then <img src=x onerror="alert(1)"> and javascript:alert(1).';
    const { container } = render(<Linkify text={text} />);
    const a = within(container).getByRole("link", { name: "https://ci.example.com/builds/812" });
    expect(a).toHaveAttribute("href", "https://ci.example.com/builds/812");
    expect(a).toHaveAttribute("rel", "noreferrer noopener");
    expect(container.querySelector("img")).toBeNull();
    expect(container).toHaveTextContent(text);
    expect(within(container).getAllByRole("link")).toHaveLength(1);
  });

  it("labels the task's own why as context", () => {
    const { container } = render(<TaskWhy text="the owner asked in https://slack.example.com/archives/C0CHECKOUT" />);
    expect(within(container).getByText(/Context:/)).toBeInTheDocument();
    expect(within(container).getByRole("link", { name: "https://slack.example.com/archives/C0CHECKOUT" })).toHaveAttribute("target", "_blank");
  });
});
