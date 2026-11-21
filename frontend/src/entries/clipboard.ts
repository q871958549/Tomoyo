import '../styles/tokens.css';
import '../styles/workbench.css';
import { mount } from 'svelte';
import ClipboardApp from '../apps/clipboard/ClipboardApp.svelte';

mount(ClipboardApp, { target: document.getElementById('app')! });
