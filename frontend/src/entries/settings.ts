import '../styles/tokens.css';
import { mount } from 'svelte';
import SettingsApp from '../apps/settings/SettingsApp.svelte';

mount(SettingsApp, { target: document.getElementById('app')! });
