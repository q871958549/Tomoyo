import '../styles/tokens.css';
import '../styles/workbench.css';
import { mount } from 'svelte';
import OcrApp from '../apps/ocr/OcrApp.svelte';

mount(OcrApp, { target: document.getElementById('app')! });
