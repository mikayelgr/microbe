import { describe, expect, it } from "vitest";
import { LineBuffer, parseLine } from "./parse";

describe("parseLine", () => {
  it("parses unlabeled values", () => {
    expect(parseLine("1.5 2 -3")).toEqual({ "1": 1.5, "2": 2, "3": -3 });
    expect(parseLine("1,2.5e3")).toEqual({ "1": 1, "2": 2500 });
  });
  it("parses labeled values", () => {
    expect(parseLine("temp:21.5 hum:40")).toEqual({ temp: 21.5, hum: 40 });
    expect(parseLine("a=1, b=2")).toEqual({ a: 1, b: 2 });
  });
  it("rejects text", () => {
    expect(parseLine("hello world")).toBeNull();
    expect(parseLine(">>> 1")).toBeNull();
    expect(parseLine("temp:hot")).toBeNull();
    expect(parseLine("")).toBeNull();
  });
});

describe("LineBuffer", () => {
  it("joins partial lines across chunks", () => {
    const b = new LineBuffer();
    const enc = new TextEncoder();
    expect(b.push(enc.encode("1 2\r\n3 "))).toEqual(["1 2"]);
    expect(b.push(enc.encode("4\n"))).toEqual(["3 4"]);
  });
});
