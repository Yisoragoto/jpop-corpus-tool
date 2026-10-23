// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { LyricsUpgradePanel } from "./LyricsUpgradePanel";
import { api, type LyricsUpgradeProgress } from "../api";

vi.mock("../events", () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock("../api", () => ({ LYRICS_SOURCE_LABELS: {qq:"QQ"}, api: { lyricsUpgradeStatus:vi.fn(), lyricsUpgradeStart:vi.fn(async()=>{}), lyricsUpgradeCancel:vi.fn(async()=>{}) } }));
let root: Root, host: HTMLDivElement;
const idle: LyricsUpgradeProgress = {running:false,cancelled:false,total:0,done:0,upgraded:0,lineFilled:0,skipped:0,notFound:0,failed:0,title:"",error:"",results:[]};
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT",true);
  host=document.createElement("div"); document.body.append(host);root=createRoot(host);
  vi.mocked(api.lyricsUpgradeStatus).mockResolvedValue({...idle});
});
afterEach(async()=>{await act(async()=>root.unmount());host.remove();vi.clearAllMocks();vi.unstubAllGlobals();});
it("starts a real batch once and restores its running state after the call",async()=>{
  const render=()=>root.render(createElement(LyricsUpgradePanel,{onChanged:vi.fn(),onError:vi.fn()}));
  await act(async()=>{render();});
  vi.mocked(api.lyricsUpgradeStatus).mockResolvedValue({...idle,running:true,total:12,title:"試験曲"});
  await act(async()=>{host.querySelector("button")!.click();host.querySelector("button")!.click();});
  expect(api.lyricsUpgradeStart).toHaveBeenCalledTimes(1);
  expect(host.querySelector("progress")?.max).toBe(12);
  expect(host.textContent).toContain("0 / 12");
  await act(async()=>{host.querySelector("button")!.click();});
  expect(api.lyricsUpgradeCancel).toHaveBeenCalledTimes(1);
});
it("mounting another page shows the backend's existing queue instead of starting it again",async()=>{
  vi.mocked(api.lyricsUpgradeStatus).mockResolvedValue({...idle,running:true,total:9,done:3,upgraded:2,lineFilled:1,title:"進行中"});
  await act(async()=>{root.render(createElement(LyricsUpgradePanel,{allowStart:false,onChanged:vi.fn(),onError:vi.fn()}));});
  expect(host.textContent).toContain("3 / 9"); expect(api.lyricsUpgradeStart).not.toHaveBeenCalled();
});
