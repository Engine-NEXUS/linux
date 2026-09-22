import { create } from "zustand";

/**
 * Sidebar store — shared state for the NEXUS Response Sidebar window.
 *
 * Supports:
 * - Response text and user query
 * - Font size customization (saved to localStorage)
 * - TTS playback tracking
 * - Lightbox modal for enlarged image viewing
 * - Copy confirmation feedback
 */

export type SidebarFontSize = "sm" | "md" | "lg" | "xl";

/** GitHub merge conflict data for the conflict panel. */
export interface ConflictData {
  prNumber: number;
  repo: string;
  conflictFiles: {
    filename: string;
    conflict_count: number;
    blocks: {
      start_line: number;
      head_content: string;
      branch_content: string;
    }[];
  }[];
  message: string;
}

export interface RepoAnalysis {
  repo: string;
  visibility: string;
  description: string;
  stars: number;
  forks: number;
  totalFiles: number;
  languages: { name: string; bytes: number; percentage: number }[];
  frameworks: { name: string; category: string }[];
  databases: { name: string; evidence: string }[];
  features: string[];
  tests: boolean;
  ci: string;
  docker: boolean;
  architecture: string;
  defaultBranch: string;
}

export interface ConfirmationData {
  requestId: string;
  prompt: string;
  command: any;
}

interface SidebarState {
  visible: boolean;
  response: string;
  query: string;
  timestamp: number;
  fontSize: SidebarFontSize;
  speaking: boolean;
  activeImage: { src: string; alt: string } | null;
  collapsedQuery: boolean;
  analysisData: RepoAnalysis | null;
  conflictData: ConflictData | null;
  confirmationData: ConfirmationData | null;

  show: (query: string, text: string) => void;
  showAnalysis: (query: string, text: string, analysis: RepoAnalysis) => void;
  showConflict: (data: ConflictData) => void;
  showConfirmation: (data: ConfirmationData) => void;
  hide: () => void;
  setFontSize: (size: SidebarFontSize) => void;
  setSpeaking: (speaking: boolean) => void;
  setActiveImage: (image: { src: string; alt: string } | null) => void;
  setCollapsedQuery: (collapsed: boolean) => void;
}

const SAVED_FONT_SIZE = (localStorage.getItem("nexus_sidebar_font_size") as SidebarFontSize) || "md";

export const useSidebar = create<SidebarState>((set) => ({
  visible: false,
  response: "",
  query: "",
  timestamp: Date.now(),
  fontSize: SAVED_FONT_SIZE,
  speaking: false,
  activeImage: null,
  collapsedQuery: false,
  analysisData: null,
  conflictData: null,
  confirmationData: null,

  show: (query: string, text: string) => {
    console.log("[sidebarStore] show called: query=", query?.substring(0, 50), "text=", text?.substring(0, 50));
    set({
      visible: true,
      query,
      response: text,
      timestamp: Date.now(),
      speaking: false,
      activeImage: null,
      analysisData: null,
      conflictData: null,
      confirmationData: null,
    });
  },

  showAnalysis: (query: string, text: string, analysis: RepoAnalysis) => {
    console.log("[sidebarStore] showAnalysis called: query=", query?.substring(0, 50), "text=", text?.substring(0, 50));
    set({
      visible: true,
      query,
      response: text,
      timestamp: Date.now(),
      speaking: false,
      activeImage: null,
      analysisData: analysis,
      conflictData: null,
      confirmationData: null,
    });
  },

  showConflict: (data: ConflictData) => {
    set({
      visible: true,
      query: `Merge Conflict — PR #${data.prNumber}`,
      response: data.message,
      timestamp: Date.now(),
      speaking: false,
      activeImage: null,
      analysisData: null,
      conflictData: data,
      confirmationData: null,
    });
  },

  showConfirmation: (data: ConfirmationData) => {
    console.log("[sidebarStore] showConfirmation called:", data);
    set({
      visible: true,
      query: "Action Confirmation",
      response: data.prompt,
      timestamp: Date.now(),
      speaking: false,
      activeImage: null,
      analysisData: null,
      conflictData: null,
      confirmationData: data,
    });
  },

  hide: () =>
    set({
      visible: false,
      speaking: false,
      activeImage: null,
      analysisData: null,
      conflictData: null,
      confirmationData: null,
    }),

  setFontSize: (size: SidebarFontSize) => {
    localStorage.setItem("nexus_sidebar_font_size", size);
    set({ fontSize: size });
  },

  setSpeaking: (speaking: boolean) => set({ speaking }),

  setActiveImage: (activeImage) => set({ activeImage }),

  setCollapsedQuery: (collapsedQuery) => set({ collapsedQuery }),
}));
