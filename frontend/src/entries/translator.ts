import '../styles/tokens.css';
import '../styles/workbench.css';
import { mount } from 'svelte';
import TranslatorApp from '../apps/translator/TranslatorApp.svelte';

mount(TranslatorApp, { target: document.getElementById('app')! });
