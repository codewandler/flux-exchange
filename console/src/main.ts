import { createApp } from 'vue'
import App from './App.vue'
import { initTheme } from './theme'
import './app.css'
import '@vue-flow/core/dist/style.css'
import '@vue-flow/core/dist/theme-default.css'
import './workflows.css'
import './channels.css'

// Before mount, so the first paint is already in the right theme rather than flashing light.
initTheme()

createApp(App).mount('#app')
