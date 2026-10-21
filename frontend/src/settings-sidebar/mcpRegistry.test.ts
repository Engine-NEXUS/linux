import { describe, expect, it } from "vitest";
import { MCP_REGISTRY } from "./McpServersPanel";
import { McpIcons } from "./McpIcons";

describe("MCP_REGISTRY", () => {
  it("contains all 10 iconic and essential MCP servers", () => {
    const ids = MCP_REGISTRY.map((s) => s.id);
    expect(ids).toContain("figma");
    expect(ids).toContain("vercel");
    expect(ids).toContain("supabase");
    expect(ids).toContain("stripe");
    expect(ids).toContain("slack");
    expect(ids).toContain("netlify");
    expect(ids).toContain("render");
    expect(ids).toContain("github");
    expect(ids).toContain("whatsapp");
    expect(ids).toContain("swiggy");
    expect(MCP_REGISTRY.length).toBe(10);
  });

  it("every server has an authentic SVG brand icon defined in McpIcons", () => {
    for (const server of MCP_REGISTRY) {
      expect(McpIcons[server.icon]).toBeDefined();
      expect(typeof McpIcons[server.icon]).toBe("function");
    }
  });

  it("deployment platforms (Vercel, Netlify, Render, Supabase) are explicitly marked", () => {
    const deploymentServers = MCP_REGISTRY.filter((s) => s.isDeploymentPlatform).map((s) => s.id);
    expect(deploymentServers).toEqual(["vercel", "supabase", "netlify", "render"]);
  });

  it("every server exposes non-empty default tools", () => {
    for (const server of MCP_REGISTRY) {
      expect(server.defaultTools.length).toBeGreaterThan(0);
    }
  });
});
