export type NoteFilter = 'all' | 'pinned' | 'todo' | 'archived';

export type EditorViewMode = 'edit' | 'split' | 'preview';

export interface NoteBacklink {
  file_name: string;
  title: string;
}

export interface NoteItem {
  file_name: string;
  path: string;
  title: string;
  snippet: string;
  body: string;
  tags: string[];
  pinned: boolean;
  archived: boolean;
  created_secs: number;
  modified_secs: number;
  source: string;
  todo_open: number;
  todo_done: number;
  wikilinks: string[];
  backlinks: NoteBacklink[];
}

export interface TagCount {
  tag: string;
  count: number;
}

export interface NotesListRes {
  notes_dir?: string;
  default_tag?: string;
  total?: number;
  pinned_count?: number;
  todo_notes_count?: number;
  archived_count?: number;
  focus_file?: string;
  tags?: TagCount[];
  notes?: NoteItem[];
}

export interface NoteMutateRes {
  ok?: boolean;
  action?: string;
  pinned?: boolean;
  file_name?: string;
  json?: string;
  note?: NoteItem;
}
