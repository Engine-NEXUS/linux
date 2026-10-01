import { create } from "zustand";

/** A single PR summary for list display. */
export interface PrSummary {
  number: number;
  title: string;
  author: string;
  repo: string;
  state: string;
  created_at: string;
  mergeable?: boolean | null;
}

interface PrListState {
  visible: boolean;
  repo: string;
  state: string;
  prs: PrSummary[];
  loading: boolean;
  actionInProgress: { prNumber: number; action: "merge" | "analyse" } | null;

  showPrList: (repo: string, state: string, prs: PrSummary[]) => void;
  hide: () => void;
  setLoading: (loading: boolean) => void;
  setActionInProgress: (action: { prNumber: number; action: "merge" | "analyse" } | null) => void;
  removePr: (prNumber: number, repo?: string) => void;
}

export const usePrList = create<PrListState>((set) => ({
  visible: false,
  repo: "",
  state: "",
  prs: [],
  loading: false,
  actionInProgress: null,

  showPrList: (repo, state, prs) => {
    set({ visible: true, repo, state, prs, loading: false, actionInProgress: null });
  },

  hide: () => set({ visible: false, prs: [], actionInProgress: null }),

  setLoading: (loading) => set({ loading }),

  setActionInProgress: (action) => set({ actionInProgress: action }),

  removePr: (prNumber, repo) =>
    set((s) => ({
      prs: s.prs.filter((p) =>
        repo ? !(p.number === prNumber && p.repo === repo) : p.number !== prNumber
      ),
    })),
}));
