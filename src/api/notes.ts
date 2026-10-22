import { invoke } from "@tauri-apps/api/core";
import type { Note } from "@/types";

export const getNotes = () => invoke<Note[]>("get_notes");
export const saveNotes = (notes: Note[]) => invoke<void>("save_notes", { notes });
