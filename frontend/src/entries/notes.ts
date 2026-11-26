import '../styles/tokens.css';
import '../styles/workbench.css';
import { mount } from 'svelte';
import NotesApp from '../apps/notes/NotesApp.svelte';

mount(NotesApp, { target: document.getElementById('app')! });
