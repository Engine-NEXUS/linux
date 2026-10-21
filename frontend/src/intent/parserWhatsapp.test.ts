import { describe, it, expect } from "vitest";
import { parseIntent } from "./parser";

describe("TS fallback parser: WhatsApp chat open", () => {
  const chat = (t: string) => {
    const i = parseIntent(t);
    return i.action === "whatsapp_chat" ? i.contact : null;
  };

  it("opens the chat instead of treating 'chat with X' as an app name", () => {
    expect(chat("open chat with mummy")).toBe("mummy");
    expect(chat("open my chat with mummy in whatsapp")).toBe("mummy");
    expect(chat("chat with mummy in whatsapp")).toBe("mummy");
    expect(chat("open mummy chat in whatsapp")).toBe("mummy");
    expect(chat("open mummy's chat on whatsapp")).toBe("mummy");
    expect(chat("open thanmatee akka chat in whatsapp")).toBe("thanmatee akka");
    expect(chat("show my chat with akka please")).toBe("akka");
  });

  it("does not hijack app opens", () => {
    expect(parseIntent("open whatsapp").action).toBe("open_app");
    expect(parseIntent("open notepad").action).toBe("open_app");
    expect(chat("open whatsapp web")).toBeNull();
  });

  it("leaves send-shaped phrases to the Rust orchestrator", () => {
    expect(chat("open chat with mummy saying hello")).toBeNull();
  });
});
