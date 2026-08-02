/**
 * The sidebar's sections, their icon tiles, and how they are grouped.
 *
 * Gradients are the exact stops from ui-spec 3.3 (MATRIX SET-013). They are
 * the one place in the UI that carries a literal colour, because they are
 * *content* — a fixed brand-ish palette identifying a section — rather than
 * theme, and they do not flip with the system appearance. Everything else
 * comes from `app.css`.
 *
 * They are also macOS-only. Windows 11 Settings inks its navigation glyphs
 * flat in the row's own colour, so `SectionIcon` hides the plate there and the
 * stops below go unused; see the `[data-platform="windows"]` rules in that
 * component.
 */

export type SectionId =
  | "general"
  | "dictation"
  | "transcription"
  | "history"
  | "dictionary"
  | "notes"
  | "privacy"
  | "system";

export interface Section {
  id: SectionId;
  title: string;
  /** Gradient top stop, bottom stop. Drawn on macOS only. */
  gradient: readonly [string, string];
}

const GRAY = ["#A3A3B3", "#7A7A8C"] as const;
const BLUE = ["#4D91FF", "#2461F5"] as const;
const ORANGE = ["#FFAD38", "#FA8005"] as const;
const GREEN = ["#57D170", "#33B34D"] as const;
const YELLOW = ["#FFD62E", "#FAB30A"] as const;
/** Teal distinguishes the Deepgram pane from adjacent navigation tiles. */
const TEAL = ["#3FD7C8", "#12A79B"] as const;

export const SECTIONS: Record<SectionId, Section> = {
  general: { id: "general", title: "General", gradient: GRAY },
  dictation: { id: "dictation", title: "Dictation", gradient: BLUE },
  transcription: { id: "transcription", title: "Transcription", gradient: TEAL },
  history: { id: "history", title: "History", gradient: ORANGE },
  dictionary: { id: "dictionary", title: "Dictionary", gradient: GREEN },
  notes: { id: "notes", title: "Notes", gradient: YELLOW },
  privacy: { id: "privacy", title: "Privacy", gradient: BLUE },
  system: { id: "system", title: "System", gradient: GRAY },
};

/** Unlabeled groups rendered as separator gaps. */
export const SECTION_GROUPS: ReadonlyArray<readonly SectionId[]> = [
  ["general", "dictation", "transcription"],
  ["history", "dictionary", "notes"],
  ["privacy", "system"],
];

/** The three sections whose view fills the detail pane edge to edge. */
const EDGE_TO_EDGE: Partial<Record<SectionId, true>> = {
  history: true,
  dictionary: true,
  notes: true,
};

export function isEdgeToEdge(id: SectionId): boolean {
  return EDGE_TO_EDGE[id] === true;
}

export const DEFAULT_SECTION: SectionId = "general";

export function isSectionId(value: string): value is SectionId {
  return value in SECTIONS;
}
