import type { Page } from "@playwright/test";
import axe from "axe-core";

type Rect = { x: number; y: number; width: number; height: number };
type Restore = { id: number; style: string | null };
type Target = {
  id: string;
  text: string;
  color: string;
  rects: Rect[];
  groups: { id: number; opacity: number }[];
};
type Preparation = { targets: Target[]; groups: number; restore: Restore[] };
export type PaintRecord = {
  id: string;
  text: string;
  verdict: "pass" | "fail" | "unknown";
  ratio: number | null;
  samples: number;
  reason?: string;
  detail?: string;
};

type AxeColor = {
  red: number;
  green: number;
  blue: number;
  alpha: number;
  parseString(value: string): AxeColor;
};
type AxeColorConstructor = new (red?: number, green?: number, blue?: number, alpha?: number) => AxeColor;
type AxeWindow = {
  axe: {
    commons: {
      color: {
        Color: AxeColorConstructor;
        getContrast(background: AxeColor, foreground: AxeColor): number | null;
      };
    };
  };
};

// Measure the browser's actual paint, not a CSS-only estimate. Captures and
// temporary styles have browser-page scope and are restored before this returns.
export async function measurePaint(page: Page, scope = "main"): Promise<PaintRecord[]> {
  await page.evaluate(axe.source);
  const prepared = await page.evaluate((scopeSelector): Preparation => {
    const root = document.querySelector<HTMLElement>(scopeSelector);
    if (!root) throw new Error(`BLOCKED: paint scope missing: ${scopeSelector}`);
    const candidates = [root, ...root.querySelectorAll<HTMLElement>("*")];
    const targets: Target[] = [];
    const restore: Restore[] = [];
    const groups: HTMLElement[] = [];
    const affected = new Set<HTMLElement>();
    const ownText = (element: HTMLElement) => [...element.childNodes].some((node) =>
      node.nodeType === Node.TEXT_NODE && Boolean(node.textContent?.trim()));
    const { Color } = (window as unknown as AxeWindow).axe.commons.color;
    const textColor = (element: HTMLElement) => {
      const style = getComputedStyle(element);
      const fill = style.getPropertyValue("-webkit-text-fill-color");
      return fill && fill !== "currentcolor" ? fill : style.color;
    };
    const elements = candidates.filter((element) =>
      ownText(element) && element.getClientRects().length > 0
      && new Color().parseString(textColor(element)).alpha > 0
      && !element.closest(".sr-only,[hidden],[aria-hidden=true]")
      && !element.closest("textarea,code"));
    for (const element of elements) {
      affected.add(element);
      for (let group: HTMLElement | null = element; group && group !== root.parentElement; group = group.parentElement) {
        const opacity = Number(getComputedStyle(group).opacity);
        if (opacity < 1) {
          if (!groups.includes(group)) groups.push(group);
          affected.add(group);
        }
      }
    }
    [...affected].forEach((element, id) => {
      element.setAttribute("data-paint-restore", String(id));
      restore.push({ id, style: element.getAttribute("style") });
    });
    for (const [id, element] of elements.entries()) {
      const rects: Rect[] = [];
      for (const node of element.childNodes) {
        if (node.nodeType !== Node.TEXT_NODE || !node.textContent?.trim()) continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        for (const rect of range.getClientRects()) {
          if (rect.width > 0 && rect.height > 0) {
            rects.push({ x: rect.x + scrollX, y: rect.y + scrollY, width: rect.width, height: rect.height });
          }
        }
      }
      const targetGroups = groups.map((group, id) => ({ group, id })).filter(({ group }) => group === element || group.contains(element));
      targetGroups.sort((a, b) => Number(a.group.contains(b.group)) - Number(b.group.contains(a.group)));
      const color = textColor(element);
      element.setAttribute("data-paint-target", String(id));
      element.style.setProperty("color", "transparent", "important");
      element.style.setProperty("-webkit-text-fill-color", "transparent", "important");
      element.style.setProperty("text-shadow", "none", "important");
      targets.push({
        id: `paint-${id}`,
        text: element.textContent?.trim() ?? "",
        color,
        rects,
        groups: targetGroups.map(({ id }) => ({ id, opacity: Number(getComputedStyle(groups[id]).opacity) })),
      });
    }
    groups.forEach((group, id) => group.setAttribute("data-paint-group", String(id)));
    return { targets, groups: groups.length, restore };
  }, scope);

  const images: string[] = [];
  try {
    if (prepared.targets.length > 0) {
      const capture = async () => {
        const layout = await page.evaluate(() => [...document.querySelectorAll<HTMLElement>("[data-paint-target]")].map((element) => {
          const rects: Rect[] = [];
          for (const node of element.childNodes) {
            if (node.nodeType !== Node.TEXT_NODE || !node.textContent?.trim()) continue;
            const range = document.createRange();
            range.selectNodeContents(node);
            for (const rect of range.getClientRects()) rects.push({ x: rect.x + scrollX, y: rect.y + scrollY, width: rect.width, height: rect.height });
          }
          return { id: `paint-${element.getAttribute("data-paint-target")}`, rects };
        }));
        if (JSON.stringify(layout) !== JSON.stringify(prepared.targets.map(({ id, rects }) => ({ id, rects })))) {
          throw new Error("BLOCKED: paint target layout changed during capture");
        }
        return (await page.screenshot({ fullPage: true, animations: "disabled" })).toString("base64");
      };
      images.push(await capture());
      for (let id = 0; id < prepared.groups; id++) {
        const group = page.locator(`[data-paint-group="${id}"]`);
        await group.evaluate((element: HTMLElement) => element.style.setProperty("visibility", "hidden", "important"));
        images.push(await capture());
        // The final cleanup below restores every original inline style. Keep the
        // group hidden only for its own backdrop capture.
        await group.evaluate((element: HTMLElement) => element.style.removeProperty("visibility"));
      }
    }
  } finally {
    await page.evaluate((items: Restore[]) => {
      for (const item of items) {
        const element = document.querySelector<HTMLElement>(`[data-paint-restore="${item.id}"]`);
        if (!element) continue;
        if (item.style === null) element.removeAttribute("style");
        else element.setAttribute("style", item.style);
      }
      document.querySelectorAll("[data-paint-target],[data-paint-group],[data-paint-restore]").forEach((element) => {
        element.removeAttribute("data-paint-target");
        element.removeAttribute("data-paint-group");
        element.removeAttribute("data-paint-restore");
      });
    }, prepared.restore);
  }
  if (prepared.targets.length === 0) return [];

  return page.evaluate(async ({ targets, images }): Promise<PaintRecord[]> => {
    const { Color, getContrast } = (window as unknown as AxeWindow).axe.commons.color;
    const decode = async (encoded: string) => {
      const bytes = Uint8Array.from(atob(encoded), (char) => char.charCodeAt(0));
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const canvas = document.createElement("canvas");
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const context = canvas.getContext("2d", { willReadFrequently: true });
      if (!context) throw new Error("BLOCKED: browser canvas decoder unavailable");
      context.drawImage(bitmap, 0, 0);
      const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height);
      bitmap.close();
      return pixels;
    };
    const decoded = [] as ImageData[];
    for (const image of images) decoded.push(await decode(image));
    const base = decoded[0];
    if (decoded.some((image) => image.width < base.width || image.height < base.height)) {
      const dimensions = decoded.map((image, index) => `${index}:${image.width}x${image.height}`).join(",");
      return targets.map((target) => ({ id: target.id, text: target.text, verdict: "unknown", ratio: null, samples: 0, reason: `capture-lost-area:${dimensions}` }));
    }
    for (const [index, image] of decoded.entries()) {
      if (image.width === base.width && image.height === base.height) continue;
      // Crop only surplus outer rows/columns. The layout check above proves no
      // target moved; a smaller capture remains a failure, never a guessed pass.
      const data = new Uint8ClampedArray(base.width * base.height * 4);
      for (let y = 0; y < base.height; y++) {
        const start = y * image.width * 4;
        data.set(image.data.subarray(start, start + base.width * 4), y * base.width * 4);
      }
      decoded[index] = new ImageData(data, base.width, base.height);
    }
    const pixel = (image: ImageData, x: number, y: number): AxeColor | null => {
      const px = Math.floor(x), py = Math.floor(y);
      if (px < 0 || py < 0 || px >= image.width || py >= image.height) return null;
      const offset = (py * image.width + px) * 4;
      return new Color(image.data[offset], image.data[offset + 1], image.data[offset + 2], image.data[offset + 3] / 255);
    };
    return targets.filter((target) => new Color().parseString(target.color).alpha > 0).map((target): PaintRecord => {
      let foreground: AxeColor;
      try {
        foreground = new Color().parseString(target.color);
      } catch {
        return { id: target.id, text: target.text, verdict: "unknown", ratio: null, samples: 0, reason: "foreground-color-parse" };
      }
      if (foreground.alpha !== 1 && target.groups.length > 0) {
        return { id: target.id, text: target.text, verdict: "unknown", ratio: null, samples: 0, reason: "translucent-group-unsupported" };
      }
      let worst: number | null = null;
      let detail: string | undefined;
      let samples = 0;
      for (const rect of target.rects) {
        for (let y = rect.y; y < rect.y + rect.height; y += 1) {
          for (let x = rect.x; x < rect.x + rect.width; x += 1) {
            const background = pixel(decoded[0], x, y);
            if (!background || background.alpha !== 1) {
              const detail = JSON.stringify({ x, y, rect, width: decoded[0].width, height: decoded[0].height, rgba: background ? [background.red, background.green, background.blue, background.alpha] : null });
              return { id: target.id, text: target.text, verdict: "unknown", ratio: null, samples, reason: `background-unavailable:${detail}` };
            }
            let effective = new Color(foreground.red, foreground.green, foreground.blue, foreground.alpha);
            for (const group of target.groups) {
              const backdrop = pixel(decoded[group.id + 1], x, y);
              if (!backdrop || backdrop.alpha !== 1) {
                return { id: target.id, text: target.text, verdict: "unknown", ratio: null, samples, reason: "opacity-backdrop-unavailable" };
              }
              effective = new Color(
                effective.red * group.opacity + backdrop.red * (1 - group.opacity),
                effective.green * group.opacity + backdrop.green * (1 - group.opacity),
                effective.blue * group.opacity + backdrop.blue * (1 - group.opacity),
              );
            }
            const ratio = getContrast(background, effective);
            if (ratio === null || !Number.isFinite(ratio)) {
              return { id: target.id, text: target.text, verdict: "unknown", ratio: null, samples, reason: "contrast-unavailable" };
            }
            if (worst === null || ratio < worst) {
              worst = ratio;
              detail = JSON.stringify({
                x, y, target: target.color,
                background: [background.red, background.green, background.blue, background.alpha],
                effective: [effective.red, effective.green, effective.blue, effective.alpha],
              });
            }
            samples += 1;
          }
        }
      }
      if (worst === null || samples === 0) {
        return { id: target.id, text: target.text, verdict: "unknown", ratio: null, samples, reason: "no-text-pixels" };
      }
      return { id: target.id, text: target.text, verdict: worst >= 4.5 ? "pass" : "fail", ratio: worst, samples, detail };
    });
  }, { targets: prepared.targets, images });
}
