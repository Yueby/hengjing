<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { onMounted, onUnmounted } from 'vue'
import AppContent from './components/AppContent.vue'
import { useAppManager } from './composables/useAppManager'
import { useEventHandlers } from './composables/useEventHandlers'

// 使用封装的应用管理器
const {
  naiveTheme,
  mcpRequest,
  showMcpPopup,
  appConfig,
  isInitializing,
  actions,
} = useAppManager()

// 创建事件处理器
const handlers = useEventHandlers(actions)

// 主题应用由useTheme统一管理，移除重复的主题应用逻辑

function handleGlobalKeydown(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 'i') {
    event.preventDefault()
    event.stopPropagation()

    invoke('open_main_devtools').catch((error) => {
      console.error('打开开发者工具失败:', error)
    })
  }
}

// 初始化
onMounted(async () => {
  try {
    window.addEventListener('keydown', handleGlobalKeydown)
    await actions.app.initialize()
  }
  catch (error) {
    console.error('应用初始化失败:', error)
  }
})

// 清理
onUnmounted(() => {
  window.removeEventListener('keydown', handleGlobalKeydown)
  actions.app.cleanup()
})
</script>

<template>
  <div class="min-h-screen bg-surface transition-colors duration-200">
    <n-config-provider :theme="naiveTheme">
      <n-message-provider>
        <n-notification-provider>
          <n-dialog-provider>
            <AppContent
              :mcp-request="mcpRequest" :show-mcp-popup="showMcpPopup" :app-config="appConfig"
              :is-initializing="isInitializing" @mcp-response="handlers.onMcpResponse" @mcp-cancel="handlers.onMcpCancel"
              @theme-change="handlers.onThemeChange" @toggle-always-on-top="handlers.onToggleAlwaysOnTop"
              @toggle-audio-notification="handlers.onToggleAudioNotification"
              @update-audio-url="handlers.onUpdateAudioUrl" @test-audio="handlers.onTestAudio"
              @stop-audio="handlers.onStopAudio" @test-audio-error="handlers.onTestAudioError"
              @update-window-size="handlers.onUpdateWindowSize"
              @update-reply-config="handlers.onUpdateReplyConfig" @message-ready="handlers.onMessageReady"
              @config-reloaded="handlers.onConfigReloaded"
            />
          </n-dialog-provider>
        </n-notification-provider>
      </n-message-provider>
    </n-config-provider>
  </div>
</template>
