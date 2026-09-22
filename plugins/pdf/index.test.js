// The plugin's own tests (Plan §53): the PDF is written here, byte by byte, with no library and
// nothing from the network.
import { describe, expect, it } from "vitest";
import { a4Box, base64Of, pdfOf } from "./dist/index.js";

const text = (bytes) => new TextDecoder("latin1").decode(bytes);
/** A tiny thing that stands in for a JPEG: the writer only carries its bytes. */
const jpeg = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0xff, 0xd9]);

describe("images to PDF", () => {
  it("fits a picture in an A4 page, centred, with a margin", () => {
    const wide = a4Box({ width: 2000, height: 1000 });
    expect(wide.page).toEqual({ width: 595, height: 842 });
    expect(wide.width).toBeCloseTo(595 - 48, 0);
    expect(wide.height).toBeCloseTo((595 - 48) / 2, 0);
    expect(wide.x).toBeCloseTo(24, 0);
    expect(wide.y).toBeCloseTo((842 - wide.height) / 2, 0);

    const tall = a4Box({ width: 1000, height: 2000 });
    expect(tall.height).toBeCloseTo(842 - 48, 0);
    expect(tall.x).toBeGreaterThan(24);
  });

  it("writes a PDF with one page per picture", () => {
    const made = text(pdfOf([{ jpeg, width: 100, height: 50 }, { jpeg, width: 50, height: 100 }]));
    expect(made.startsWith("%PDF-1.4")).toBe(true);
    expect(made).toContain("/Count 2");
    expect(made.match(/\/Type \/Page[^s]/g)).toHaveLength(2);
    expect(made).toContain("/Filter /DCTDecode");
    expect(made.trimEnd().endsWith("%%EOF")).toBe(true);
  });

  // A reader finds the objects through the table: a wrong offset is an unopenable file.
  it("points the table at where each object really is", () => {
    const bytes = pdfOf([{ jpeg, width: 100, height: 50 }]);
    const made = text(bytes);
    const start = Number(made.slice(made.lastIndexOf("startxref")).split("\n")[1]);
    expect(made.slice(start, start + 4)).toBe("xref");

    const table = made.slice(start).split("\n").slice(2);
    const objects = Number(made.slice(start).split("\n")[1].split(" ")[1]);
    for (let object = 1; object < objects; object += 1) {
      const offset = Number(table[object].slice(0, 10));
      expect(made.slice(offset, offset + `${object} 0 obj`.length)).toBe(`${object} 0 obj`);
    }
  });

  it("keeps the bytes of the picture as they were", () => {
    const bytes = pdfOf([{ jpeg, width: 4, height: 4 }]);
    const made = text(bytes);
    const at = made.indexOf("stream", made.indexOf("/DCTDecode")) + "stream\n".length;
    expect(Array.from(bytes.slice(at, at + jpeg.length))).toEqual(Array.from(jpeg));
  });

  it("is nothing at all without pictures", () => {
    expect(pdfOf([])).toBe(null);
  });

  it("turns bytes into base64 the app can carry", () => {
    expect(base64Of(new Uint8Array([65, 66, 67]))).toBe("QUJD");
    const long = new Uint8Array(200_000).map((_, index) => index % 251);
    expect(atob(base64Of(long)).length).toBe(long.length);
  });

  it("is a custom element the frame can show", () => {
    expect(customElements.get("ft-pdf")).toBeTruthy();
  });
});
