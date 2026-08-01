// Light and dark, the way the carried components expect it.
//
// `tokens.css` mirrors VitePress's palette, and the components' own dark rules are written against
// `.dark` on the root element — so that is where the class goes. The preference is the reader's
// first, the operating system's second; a stored choice survives a reload and an unstored one keeps
// following the OS, including when the OS changes while the page is open.

import { ref } from 'vue'

const STORAGE_KEY = 'flux-exchange-console:theme'

const media = window.matchMedia('(prefers-color-scheme: dark)')

/** Whether dark is currently applied — reactive, so the toggle's own label can read it. */
export const isDark = ref(false)

function apply(dark: boolean) {
  isDark.value = dark
  document.documentElement.classList.toggle('dark', dark)
}

/** What the reader chose, or `null` when they have not chosen and the OS still decides. */
function stored(): 'light' | 'dark' | null {
  const value = localStorage.getItem(STORAGE_KEY)
  return value === 'light' || value === 'dark' ? value : null
}

/** Set the theme from the stored choice, else from the OS, and keep following the OS until chosen. */
export function initTheme() {
  const choice = stored()
  apply(choice ? choice === 'dark' : media.matches)
  media.addEventListener('change', (event) => {
    if (!stored()) apply(event.matches)
  })
}

/** Flip the theme and remember it — from here on the OS no longer decides. */
export function toggleTheme() {
  const dark = !isDark.value
  localStorage.setItem(STORAGE_KEY, dark ? 'dark' : 'light')
  apply(dark)
}
