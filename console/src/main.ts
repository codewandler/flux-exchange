import { createApp } from 'vue'
import App from './App.vue'
import { initTheme } from './theme'
import './app.css'

// Before mount, so the first paint is already in the right theme rather than flashing light.
initTheme()

createApp(App).mount('#app')
