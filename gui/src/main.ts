// SPDX-License-Identifier: AGPL-3.0-only
import { createApp } from 'vue'
import App from './App.vue'
import { useTheme } from './composables/useTheme'

const app = createApp(App)
useTheme()
app.mount('#app')
