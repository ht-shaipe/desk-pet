import { defineStore } from "pinia";
import { ref } from "vue";
import { getNotes, saveNotes } from "@/api/notes";
import type { Note } from "@/types";

const PALETTE = ["#fef3c7", "#dcfce7", "#dbeafe", "#fce7f3", "#ede9fe"];

let persistTimer: ReturnType<typeof setTimeout> | undefined;

export const useNotesStore = defineStore("notes", () => {
  const notes = ref<Note[]>([]);
  const loaded = ref(false);

  function schedulePersist() {
    if (persistTimer !== undefined) {
      clearTimeout(persistTimer);
    }
    persistTimer = setTimeout(() => {
      void saveNotes(notes.value);
    }, 600);
  }

  async function init() {
    if (loaded.value) {
      return;
    }
    notes.value = await getNotes();
    loaded.value = true;
  }

  function addNote() {
    notes.value.unshift({
      id: `n-${Date.now()}-${Math.floor(Math.random() * 1000)}`,
      content: "",
      color: PALETTE[notes.value.length % PALETTE.length],
      updated_at: Date.now(),
    });
    schedulePersist();
  }

  function updateNote(id: string, content: string) {
    const note = notes.value.find((n) => n.id === id);
    if (note) {
      note.content = content;
      note.updated_at = Date.now();
      schedulePersist();
    }
  }

  function setNoteColor(id: string, color: string) {
    const note = notes.value.find((n) => n.id === id);
    if (note) {
      note.color = color;
      note.updated_at = Date.now();
      schedulePersist();
    }
  }

  function removeNote(id: string) {
    notes.value = notes.value.filter((n) => n.id !== id);
    schedulePersist();
  }

  return { notes, init, addNote, updateNote, setNoteColor, removeNote };
});
