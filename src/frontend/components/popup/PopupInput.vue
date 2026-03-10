<script setup lang="ts">
import type { CustomPrompt, FileReferenceAttachment, McpRequest } from '../../types/popup'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useDebounceFn } from '@vueuse/core'
import { useSortable } from '@vueuse/integrations/useSortable'
import { useMessage } from 'naive-ui'
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import { useKeyboard } from '../../composables/useKeyboard'

interface Props {
  request: McpRequest | null
  loading?: boolean
  submitting?: boolean
}

interface Emits {
  update: [data: {
    userInput: string
    selectedOptions: string[]
    draggedImages: string[]
    referencedFiles: FileReferenceAttachment[]
  }]
  imageAdd: [image: string]
  imageRemove: [index: number]
  submit: [] // 触发自动提交
}

const props = withDefaults(defineProps<Props>(), {
  loading: false,
  submitting: false,
})

const emit = defineEmits<Emits>()

// 响应式数据
const userInput = ref('')
const selectedOptions = ref<string[]>([])
const uploadedImages = ref<string[]>([])
const referencedFiles = ref<FileReferenceAttachment[]>([])
const isDragOver = ref(false)
const textareaRef = ref<HTMLDivElement | null>(null)

// 自定义prompt相关状态
const customPrompts = ref<CustomPrompt[]>([])
const customPromptEnabled = ref(true)
const defaultAppendMode = ref(false) // 默认追加模式
const showInsertDialog = ref(false)
const pendingPromptContent = ref('')

// 移除条件性prompt状态管理，直接使用prompt的current_state

// 分离普通prompt和条件性prompt
const normalPrompts = computed(() =>
  customPrompts.value.filter(prompt => prompt.type === 'normal' || !prompt.type),
)

const conditionalPrompts = computed(() =>
  customPrompts.value.filter(prompt => prompt.type === 'conditional'),
)

// 拖拽排序相关状态
const promptContainer = ref<HTMLElement | null>(null)
const sortablePrompts = shallowRef<CustomPrompt[]>([])
const { start, stop } = useSortable(promptContainer, sortablePrompts, {
  animation: 200,
  ghostClass: 'sortable-ghost',
  chosenClass: 'sortable-chosen',
  dragClass: 'sortable-drag',
  handle: '.drag-handle',
  forceFallback: true,
  fallbackTolerance: 3,
  onStart: (evt) => {
    console.log('PopupInput: 拖拽开始:', evt)
    console.log('PopupInput: 拖拽开始时的容器:', evt.from)
    console.log('PopupInput: 拖拽开始时的元素:', evt.item)
  },
  onEnd: (evt) => {
    console.log('PopupInput: 拖拽排序完成:', evt)
    console.log('PopupInput: 从索引', evt.oldIndex, '移动到索引', evt.newIndex)
    console.log('PopupInput: 拖拽后的sortablePrompts:', sortablePrompts.value.map(p => ({ id: p.id, name: p.name })))

    // 检查是否真的发生了位置变化
    if (evt.oldIndex !== evt.newIndex && evt.oldIndex !== undefined && evt.newIndex !== undefined) {
      // 手动重新排列数组
      const newList = [...sortablePrompts.value]
      const [movedItem] = newList.splice(evt.oldIndex, 1)
      newList.splice(evt.newIndex, 0, movedItem)

      // 更新sortablePrompts
      sortablePrompts.value = newList
      console.log('PopupInput: 手动更新后的sortablePrompts:', sortablePrompts.value.map(p => ({ id: p.id, name: p.name })))

      // 立即更新 customPrompts 的顺序，确保数据同步
      // 保留条件性prompt，只更新普通prompt的顺序
      const conditionalPromptsList = customPrompts.value.filter(prompt => prompt.type === 'conditional')
      customPrompts.value = [...sortablePrompts.value, ...conditionalPromptsList]
      console.log('PopupInput: 位置发生变化，保存新排序')

      // 立即保存排序
      savePromptOrder()
    }
    else {
      console.log('PopupInput: 位置未发生变化，无需保存')
    }
  },
  onMove: (evt) => {
    console.log('PopupInput: 拖拽移动中:', evt)
    return true // 允许移动
  },
  onChoose: (evt) => {
    console.log('PopupInput: 选择拖拽元素:', evt)
  },
  onUnchoose: (evt) => {
    console.log('PopupInput: 取消选择拖拽元素:', evt)
  },
})

// 使用键盘快捷键 composable
const { pasteShortcut } = useKeyboard()

const message = useMessage()

// 计算属性
const hasOptions = computed(() => (props.request?.predefined_options?.length ?? 0) > 0)
const canSubmit = computed(() => {
  const hasOptionsSelected = selectedOptions.value.length > 0
  // 直接从 textarea 读取值判断
  const hasInputText = getCurrentInputValue().trim().length > 0
  const hasImages = uploadedImages.value.length > 0
  const hasFiles = referencedFiles.value.length > 0

  if (hasOptions.value) {
    return hasOptionsSelected || hasInputText || hasImages || hasFiles
  }
  return hasInputText || hasImages || hasFiles
})

// 工具栏状态文本
const statusText = computed(() => {
  // 检查是否有任何输入内容
  const hasInput = selectedOptions.value.length > 0
    || uploadedImages.value.length > 0
    || referencedFiles.value.length > 0
    || getCurrentInputValue().trim().length > 0

  // 如果有任何输入内容，返回空字符串让 PopupActions 显示快捷键
  if (hasInput) {
    return ''
  }

  return '等待输入...'
})

// 发送更新事件（带防抖，避免大文本输入时频繁触发）
const debouncedEmitUpdate = useDebounceFn(() => {
  // 从 textarea 直接读取值，避免响应式更新
  const currentInput = getCurrentInputValue()

  // 获取条件性prompt的追加内容
  const conditionalContent = generateConditionalContent()

  // 将条件性内容追加到用户输入
  const finalUserInput = currentInput + conditionalContent

  emit('update', {
    userInput: finalUserInput,
    selectedOptions: selectedOptions.value,
    draggedImages: uploadedImages.value,
    referencedFiles: referencedFiles.value,
  })
}, 220) // 220ms 防抖

// 立即发送更新事件（用于选项变化等需要即时响应的场景）
function emitUpdateImmediate() {
  // 从 textarea 直接读取值
  const currentInput = getCurrentInputValue()

  // 获取条件性prompt的追加内容
  const conditionalContent = generateConditionalContent()

  // 将条件性内容追加到用户输入
  const finalUserInput = currentInput + conditionalContent

  console.log('[DEBUG] emitUpdateImmediate:', {
    currentInput,
    conditionalContent,
    finalUserInput,
    selectedOptions: selectedOptions.value,
    uploadedImages: uploadedImages.value.length,
  })

  emit('update', {
    userInput: finalUserInput,
    selectedOptions: selectedOptions.value,
    draggedImages: uploadedImages.value,
    referencedFiles: referencedFiles.value,
  })
}

// 发送更新事件
function emitUpdate() {
  debouncedEmitUpdate()
}

// 处理选项变化
function handleOptionChange(option: string, checked: boolean) {
  if (checked) {
    selectedOptions.value.push(option)
  }
  else {
    const idx = selectedOptions.value.indexOf(option)
    if (idx > -1)
      selectedOptions.value.splice(idx, 1)
  }
  emitUpdateImmediate() // 选项变化需要即时响应
}

// 处理选项切换（整行点击）
function handleOptionToggle(option: string) {
  const idx = selectedOptions.value.indexOf(option)
  if (idx > -1) {
    selectedOptions.value.splice(idx, 1)
  }
  else {
    selectedOptions.value.push(option)
  }
  emitUpdateImmediate() // 选项变化需要即时响应
}

// 移除了所有拖拽和上传组件相关的代码

async function handleImagePaste(event: ClipboardEvent) {
  const items = event.clipboardData?.items
  let hasImage = false

  if (items) {
    for (const item of items) {
      if (item.type.includes('image')) {
        hasImage = true
        const file = item.getAsFile()
        if (file) {
          handleImageFiles([file])
        }
      }
    }
  }

  if (hasImage) {
    event.preventDefault()
  }
  else {
    const plainText = event.clipboardData?.getData('text/plain') || ''
    event.preventDefault()
    const handledAsReference = await insertParsedContentAtCaret(plainText)
    if (!handledAsReference && plainText) {
      insertTextAtCaret(plainText)
      userInput.value = getCurrentInputValue()
      debouncedEmitUpdate()
    }

    handlePasteResize()
    return
  }
}

const imageExtensionToMediaType: Record<string, string> = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.bmp': 'image/bmp',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.avif': 'image/avif',
}

function normalizeFilePath(path: string): string {
  return path.replace(/\\/g, '/')
}

function getFileExtension(path: string): string {
  const normalizedPath = normalizeFilePath(path).toLowerCase()
  const fileName = normalizedPath.split('/').pop() || normalizedPath
  const dotIndex = fileName.lastIndexOf('.')
  return dotIndex >= 0 ? fileName.slice(dotIndex) : ''
}

function getFileNameFromPath(path: string): string {
  const normalizedPath = normalizeFilePath(path)
  return normalizedPath.split('/').pop() || normalizedPath
}

function decodeFileUri(uri: string): string | null {
  if (!uri.toLowerCase().startsWith('file://')) {
    return null
  }

  try {
    const url = new URL(uri)
    let pathname = decodeURIComponent(url.pathname)

    if (/^\/[a-zA-Z]:\//.test(pathname)) {
      pathname = pathname.slice(1)
    }

    if (url.host) {
      pathname = `//${url.host}${pathname}`
    }

    return normalizeFilePath(pathname)
  }
  catch {
    return null
  }
}

function extractPathCandidates(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map(line => line.trim())
    .filter(Boolean)
    .map(line => line.replace(/^['"]|['"]$/g, ''))
    .flatMap((line) => {
      const fileUriPath = decodeFileUri(line)
      if (fileUriPath) {
        return [fileUriPath]
      }

      if (/^(?:[a-zA-Z]:[\\/]|\/|\\\\)/.test(line)) {
        return [normalizeFilePath(line)]
      }

      return []
    })
}

function extractUrlCandidates(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map(line => line.trim())
    .filter(Boolean)
    .map(line => line.replace(/^['"]|['"]$/g, ''))
    .filter(line => /^https?:\/\/\S+$/i.test(line))
}

function getReferenceKindLabel(reference: FileReferenceAttachment): string {
  if (reference.type === 'url') {
    return 'URL'
  }

  switch (reference.kind) {
    case 'directory':
      return '目录'
    default:
      return '文件'
  }
}

function createUrlReference(url: string): FileReferenceAttachment {
  let filename = url

  try {
    const parsedUrl = new URL(url)
    const pathSegments = parsedUrl.pathname.split('/').filter(Boolean)
    filename = pathSegments[pathSegments.length - 1] || parsedUrl.hostname || url
  }
  catch {
    // ignore
  }

  return {
    type: 'url',
    url,
    name: filename,
    mime_type: null,
  }
}

function getReferenceIdentity(reference: FileReferenceAttachment): string {
  const value = reference.type === 'url' ? (reference.url || '') : (reference.path || '')
  return `${reference.type}::${value.trim().toLowerCase()}`
}

function getReferenceInlineLabel(reference: FileReferenceAttachment): string {
  const prefix = reference.type === 'url' ? '@链接' : '@文件'
  return `${prefix}:${reference.name}`
}

function getSerializedReferenceText(reference: FileReferenceAttachment): string {
  const value = reference.type === 'url' ? reference.url : reference.path
  return JSON.stringify(value || '')
}

const inlineReferencePattern = /("(?:file:\/\/|https?:\/\/|[a-zA-Z]:\\)[^"\r\n]+"|https?:\/\/[^\s"'<>]+|file:\/\/[^\s"'<>]+|[a-zA-Z]:\\[^\r\n"'<>]+)/g

function isInlineReferenceBoundary(text: string, index: number, length: number): boolean {
  const before = index > 0 ? text[index - 1] : ''
  const afterIndex = index + length
  const after = afterIndex < text.length ? text[afterIndex] : ''
  const beforeBoundary = before === '' || /[\s([{\u3000"'`]/.test(before)
  const afterBoundary = after === '' || /[\s)\]}.,;:!?\u3000"'`]/.test(after)
  return beforeBoundary && afterBoundary
}

function splitTrailingPunctuation(token: string): { core: string, suffix: string } {
  const match = token.match(/^(.*?)([.,;:!?)\]]+)?$/)
  return {
    core: match?.[1] || token,
    suffix: match?.[2] || '',
  }
}

function tokenizeReferenceContent(text: string): Array<{ type: 'text' | 'candidate', value: string }> {
  const segments: Array<{ type: 'text' | 'candidate', value: string }> = []
  let lastIndex = 0

  for (const match of text.matchAll(inlineReferencePattern)) {
    const index = match.index ?? 0
    const value = match[0]

    if (!isInlineReferenceBoundary(text, index, value.length)) {
      continue
    }

    if (index > lastIndex) {
      segments.push({ type: 'text', value: text.slice(lastIndex, index) })
    }

    const { core, suffix } = splitTrailingPunctuation(value)
    if (core) {
      segments.push({ type: 'candidate', value: core })
    }
    if (suffix) {
      segments.push({ type: 'text', value: suffix })
    }

    lastIndex = index + value.length
  }

  if (lastIndex < text.length) {
    segments.push({ type: 'text', value: text.slice(lastIndex) })
  }

  return segments
}

function guessMediaTypeFromPath(path: string): string | null {
  return imageExtensionToMediaType[getFileExtension(path)] || null
}

function isImagePath(path: string): boolean {
  return !!guessMediaTypeFromPath(path)
}

function ensureBlobMediaType(blob: Blob, fallbackPath?: string): Blob {
  const fallbackMediaType = fallbackPath ? guessMediaTypeFromPath(fallbackPath) : null
  const mediaType = blob.type || fallbackMediaType

  if (!mediaType || blob.type === mediaType) {
    return blob
  }

  return new Blob([blob], { type: mediaType })
}

async function pathToDataUrl(path: string): Promise<string> {
  return await invoke<string>('read_image_file_as_data_url', { path })
}

async function validateFileReferencePath(path: string): Promise<FileReferenceAttachment | null> {
  try {
    return await invoke<FileReferenceAttachment>('validate_file_reference_path', { path })
  }
  catch (error) {
    const errorMessage = `${(error as Error)?.message || error}`
    console.error('校验文件引用路径失败:', { path, error })
    message.error(errorMessage)
    return null
  }
}

async function resolveReferenceCandidate(candidate: string): Promise<FileReferenceAttachment | null> {
  const trimmedCandidate = candidate.trim().replace(/^['"]|['"]$/g, '')

  if (/^https?:\/\//i.test(trimmedCandidate)) {
    return createUrlReference(trimmedCandidate)
  }

  const decodedPath = decodeFileUri(trimmedCandidate) || normalizeFilePath(trimmedCandidate)
  return await validateFileReferencePath(decodedPath)
}

function addReferencedFile(file: FileReferenceAttachment): boolean {
  const normalizedPath = normalizeFilePath(file.path || '')
  const exists = referencedFiles.value.some(item => item.type === 'path' && normalizeFilePath(item.path || '').toLowerCase() === normalizedPath.toLowerCase())
  const referenceLabel = file.kind === 'directory' ? '目录' : '文件'

  if (exists) {
    message.warning(`${referenceLabel} ${file.name} 已存在`)
    return false
  }

  referencedFiles.value.push({
    ...file,
    type: 'path',
    path: normalizedPath,
    name: file.name || getFileNameFromPath(normalizedPath),
    kind: file.kind ?? 'file',
    mime_type: file.mime_type ?? guessMediaTypeFromPath(normalizedPath),
  })

  message.success(`已引用${referenceLabel} ${file.name}`)
  return true
}

function addReferencedAttachment(attachment: FileReferenceAttachment): boolean {
  if (attachment.type === 'url') {
    const normalizedUrl = (attachment.url || '').trim()
    const exists = referencedFiles.value.some(item => item.type === 'url' && (item.url || '').trim().toLowerCase() === normalizedUrl.toLowerCase())

    if (exists) {
      message.warning(`URL ${attachment.url} 已存在`)
      return false
    }

    referencedFiles.value.push({
      ...attachment,
      type: 'url',
      url: normalizedUrl,
      name: attachment.name || normalizedUrl,
      mime_type: null,
    })

    message.success(`已引用URL ${attachment.url}`)
    return true
  }

  return addReferencedFile(attachment)
}

function removeReferenceByIdentity(identity: string) {
  const referenceIndex = referencedFiles.value.findIndex(item => getReferenceIdentity(item) === identity)
  if (referenceIndex > -1) {
    referencedFiles.value.splice(referenceIndex, 1)
  }

  if (textareaRef.value) {
    const badge = textareaRef.value.querySelector(`[data-reference-id="${CSS.escape(identity)}"]`)
    if (badge) {
      const trailingSpace = badge.nextSibling
      badge.remove()
      if (trailingSpace?.nodeType === Node.TEXT_NODE && trailingSpace.textContent === ' ') {
        trailingSpace.remove()
      }
    }
  }

  userInput.value = getCurrentInputValue()
  emitUpdateImmediate()
}

function syncReferencedFilesWithEditor() {
  if (!textareaRef.value) {
    return
  }

  const activeReferenceIds = new Set(
    Array.from(textareaRef.value.querySelectorAll<HTMLElement>('[data-reference-id]'))
      .map(element => element.dataset.referenceId)
      .filter((id): id is string => !!id),
  )

  referencedFiles.value = referencedFiles.value.filter(reference =>
    activeReferenceIds.has(getReferenceIdentity(reference)),
  )
}

function createReferenceBadgeElement(reference: FileReferenceAttachment): HTMLSpanElement {
  const identity = getReferenceIdentity(reference)
  const badge = document.createElement('span')
  badge.className = 'popup-inline-reference'
  badge.contentEditable = 'false'
  badge.dataset.referenceId = identity
  badge.dataset.referenceLabel = getReferenceInlineLabel(reference)
  badge.dataset.referenceSerialized = getSerializedReferenceText(reference)
  badge.title = reference.type === 'url' ? (reference.url || '') : (reference.path || '')

  const kind = document.createElement('span')
  kind.className = 'popup-inline-reference-kind'
  kind.textContent = getReferenceKindLabel(reference)

  const label = document.createElement('span')
  label.className = 'popup-inline-reference-label'
  label.textContent = reference.name

  badge.append(kind, label)
  return badge
}

function focusEditor() {
  textareaRef.value?.focus()
}

function ensureEditorSelection() {
  if (!textareaRef.value) {
    return
  }

  focusEditor()

  const selection = window.getSelection()
  if (!selection || selection.rangeCount === 0 || !textareaRef.value.contains(selection.anchorNode)) {
    moveCaretToEnd()
  }
}

function moveCaretToEnd() {
  if (!textareaRef.value)
    return

  const selection = window.getSelection()
  if (!selection)
    return

  const range = document.createRange()
  range.selectNodeContents(textareaRef.value)
  range.collapse(false)
  selection.removeAllRanges()
  selection.addRange(range)
}

function insertNodeAtCaret(node: Node) {
  if (!textareaRef.value)
    return

  ensureEditorSelection()

  const activeSelection = window.getSelection()
  if (!activeSelection || activeSelection.rangeCount === 0)
    return

  const range = activeSelection.getRangeAt(0)
  const lastInsertedNode = node instanceof DocumentFragment ? node.lastChild : node
  range.deleteContents()
  range.insertNode(node)

  if (lastInsertedNode?.parentNode) {
    range.setStartAfter(lastInsertedNode)
    range.collapse(true)
    activeSelection.removeAllRanges()
    activeSelection.addRange(range)
  }

  focusEditor()
}

function insertTextAtCaret(text: string) {
  insertNodeAtCaret(document.createTextNode(text))
}

function findAdjacentReferenceBadge(direction: 'backward' | 'forward'): HTMLElement | null {
  if (!textareaRef.value) {
    return null
  }

  const selection = window.getSelection()
  if (!selection || selection.rangeCount === 0 || !selection.isCollapsed) {
    return null
  }

  const range = selection.getRangeAt(0)
  let container = range.startContainer
  let offset = range.startOffset

  if (container.nodeType === Node.TEXT_NODE) {
    const textNode = container as Text

    if (direction === 'backward' && offset === 0) {
      let sibling = textNode.previousSibling
      while (sibling?.nodeType === Node.TEXT_NODE && sibling.textContent === '') {
        sibling = sibling.previousSibling
      }
      return sibling instanceof HTMLElement && sibling.dataset.referenceId ? sibling : null
    }

    if (direction === 'forward' && offset === textNode.textContent!.length) {
      let sibling = textNode.nextSibling
      while (sibling?.nodeType === Node.TEXT_NODE && sibling.textContent === '') {
        sibling = sibling.nextSibling
      }
      return sibling instanceof HTMLElement && sibling.dataset.referenceId ? sibling : null
    }

    return null
  }

  if (!(container instanceof HTMLElement)) {
    return null
  }

  const childNodes = Array.from(container.childNodes)
  const targetIndex = direction === 'backward' ? offset - 1 : offset
  const candidate = childNodes[targetIndex]
  return candidate instanceof HTMLElement && candidate.dataset.referenceId ? candidate : null
}

function handleEditorKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
    event.preventDefault()
    insertTextAtCaret('\n')
    return
  }

  if (event.key !== 'Backspace' && event.key !== 'Delete') {
    return
  }

  const badge = findAdjacentReferenceBadge(event.key === 'Backspace' ? 'backward' : 'forward')
  if (!badge?.dataset.referenceId) {
    return
  }

  event.preventDefault()
  removeReferenceByIdentity(badge.dataset.referenceId)
}

function insertReferenceBadge(reference: FileReferenceAttachment) {
  const badge = createReferenceBadgeElement(reference)
  insertNodeAtCaret(badge)
  insertNodeAtCaret(document.createTextNode(' '))
  userInput.value = getCurrentInputValue()
  ensureEditorSelection()
}

function hasInlineReferenceBadge(identity: string): boolean {
  return !!textareaRef.value?.querySelector(`[data-reference-id="${CSS.escape(identity)}"]`)
}

async function insertParsedContentAtCaret(text: string): Promise<boolean> {
  const directCandidates = [...extractPathCandidates(text), ...extractUrlCandidates(text)]
  if (directCandidates.length === 1 && text.trim()) {
    const directReference = await resolveReferenceCandidate(text.trim())
    if (directReference && addReferencedAttachment(directReference)) {
      insertReferenceBadge(directReference)
      emitUpdateImmediate()
      return true
    }
  }

  const segments = tokenizeReferenceContent(text)
  if (!segments.some(segment => segment.type === 'candidate')) {
    return false
  }

  const fragment = document.createDocumentFragment()
  let insertedReference = false

  for (const segment of segments) {
    if (segment.type === 'text') {
      if (segment.value) {
        fragment.append(document.createTextNode(segment.value))
      }
      continue
    }

    const reference = await resolveReferenceCandidate(segment.value)
    if (!reference) {
      fragment.append(document.createTextNode(segment.value))
      continue
    }

    const added = addReferencedAttachment(reference)
    if (!added && !hasInlineReferenceBadge(getReferenceIdentity(reference))) {
      fragment.append(document.createTextNode(segment.value))
      continue
    }
    fragment.append(createReferenceBadgeElement(reference))
    fragment.append(document.createTextNode(' '))
    insertedReference = true
  }

  if (!insertedReference) {
    return false
  }

  insertNodeAtCaret(fragment)
  userInput.value = getCurrentInputValue()
  emitUpdateImmediate()
  return true
}

async function replaceTextNodeWithInlineReferences(textNode: Text): Promise<boolean> {
  const text = textNode.textContent || ''
  const segments = tokenizeReferenceContent(text)
  if (!segments.some(segment => segment.type === 'candidate')) {
    return false
  }

  const fragment = document.createDocumentFragment()
  let replaced = false

  for (const segment of segments) {
    if (segment.type === 'text') {
      if (segment.value) {
        fragment.append(document.createTextNode(segment.value))
      }
      continue
    }

    const reference = await resolveReferenceCandidate(segment.value)
    if (!reference) {
      fragment.append(document.createTextNode(segment.value))
      continue
    }

    const added = addReferencedAttachment(reference)
    if (!added && !hasInlineReferenceBadge(getReferenceIdentity(reference))) {
      fragment.append(document.createTextNode(segment.value))
      continue
    }

    fragment.append(createReferenceBadgeElement(reference))
    fragment.append(document.createTextNode(' '))
    replaced = true
  }

  if (!replaced) {
    return false
  }

  const selection = window.getSelection()
  const shouldRestoreCaret = !!selection
    && selection.rangeCount > 0
    && selection.isCollapsed
    && selection.anchorNode === textNode
  const lastInsertedNode = fragment.lastChild

  textNode.replaceWith(fragment)

  if (shouldRestoreCaret && lastInsertedNode?.parentNode) {
    const range = document.createRange()
    range.setStartAfter(lastInsertedNode)
    range.collapse(true)
    selection!.removeAllRanges()
    selection!.addRange(range)
    focusEditor()
  }

  return true
}

async function scanEditorForInlineReferences(): Promise<void> {
  if (!textareaRef.value || isComposing.value) {
    return
  }

  const textNodes: Text[] = []
  const walker = document.createTreeWalker(textareaRef.value, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const textNode = node as Text
      const parentElement = textNode.parentElement
      if (!textNode.textContent || !inlineReferencePattern.test(textNode.textContent)) {
        inlineReferencePattern.lastIndex = 0
        return NodeFilter.FILTER_REJECT
      }
      inlineReferencePattern.lastIndex = 0
      if (parentElement?.closest('.popup-inline-reference')) {
        return NodeFilter.FILTER_REJECT
      }
      return NodeFilter.FILTER_ACCEPT
    },
  })

  let currentNode = walker.nextNode()
  while (currentNode) {
    textNodes.push(currentNode as Text)
    currentNode = walker.nextNode()
  }

  let changed = false
  for (const textNode of textNodes) {
    if (await replaceTextNodeWithInlineReferences(textNode)) {
      changed = true
    }
  }

  if (changed) {
    userInput.value = getCurrentInputValue()
    emitUpdateImmediate()
    ensureEditorSelection()
  }
}

function setEditorPlainText(text: string) {
  if (!textareaRef.value)
    return

  textareaRef.value.innerHTML = ''
  if (text) {
    textareaRef.value.append(document.createTextNode(text))
  }
  userInput.value = getCurrentInputValue()
}

async function handleDroppedPaths(paths: string[]): Promise<void> {
  console.log('=== 处理拖拽文件 ===')
  console.log('拖拽路径:', paths)

  let hasChanges = false

  for (const rawPath of paths) {
    const normalizedPath = normalizeFilePath(rawPath)
    const validatedFile = await validateFileReferencePath(normalizedPath)

    if (!validatedFile) {
      continue
    }

    if (validatedFile.type === 'path' && validatedFile.kind !== 'directory' && isImagePath(validatedFile.path || '')) {
      try {
        const base64 = await pathToDataUrl(validatedFile.path || '')
        if (!uploadedImages.value.includes(base64)) {
          uploadedImages.value.push(base64)
          message.success(`图片 ${validatedFile.name} 已添加`)
          hasChanges = true
        }
        else {
          message.warning(`图片 ${validatedFile.name} 已存在`)
        }
      }
      catch (error) {
        console.error('拖拽图片处理失败:', error)
        message.error(`图片 ${validatedFile.name} 处理失败`)
      }
    }
    else if (addReferencedAttachment(validatedFile)) {
      insertReferenceBadge(validatedFile)
      hasChanges = true
    }
  }

  if (hasChanges) {
    emitUpdateImmediate()
    ensureEditorSelection()
  }

  console.log('=== 拖拽文件处理完成 ===')
}

async function handleImageFiles(files: FileList | File[]): Promise<void> {
  console.log('=== 处理图片文件 ===')
  console.log('文件数量:', files.length)

  for (const file of files) {
    console.log('处理文件:', file.name, '类型:', file.type, '大小:', file.size)

    if (file.type.startsWith('image/')) {
      try {
        console.log('开始转换为 Base64...')
        const base64 = await fileToBase64(ensureBlobMediaType(file, file.name))
        console.log('Base64转换成功，长度:', base64.length)

        // 检查是否已存在相同图片，避免重复添加
        if (!uploadedImages.value.includes(base64)) {
          uploadedImages.value.push(base64)
          console.log('图片已添加到数组，当前数量:', uploadedImages.value.length)
          message.success(`图片 ${file.name} 已添加`)
          // 图片添加后立即触发更新，避免防抖延迟
          emitUpdateImmediate()
        }
        else {
          console.log('图片已存在，跳过:', file.name)
          message.warning(`图片 ${file.name} 已存在`)
        }
      }
      catch (error) {
        console.error('图片处理失败:', error)
        message.error(`图片 ${file.name} 处理失败`)
        throw error
      }
    }
    else {
      console.log('跳过非图片文件:', file.type)
    }
  }

  console.log('=== 图片文件处理完成 ===')
}

function fileToBase64(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as string)
    reader.onerror = reject
    reader.readAsDataURL(file)
  })
}

function removeImage(index: number) {
  uploadedImages.value.splice(index, 1)
  emit('imageRemove', index)
  emitUpdateImmediate() // 图片变化需要即时响应
}

function removeReferencedFile(index: number) {
  referencedFiles.value.splice(index, 1)
  emitUpdateImmediate()
}

// 移除自定义图片预览功能，改用 Naive UI 的内置预览

// 加载自定义prompt配置
async function loadCustomPrompts() {
  try {
    console.log('PopupInput: 开始加载自定义prompt配置')
    const config = await invoke('get_custom_prompt_config')
    if (config) {
      const promptConfig = config as any

      // 按sort_order排序
      customPrompts.value = (promptConfig.prompts || []).sort((a: CustomPrompt, b: CustomPrompt) => a.sort_order - b.sort_order)
      customPromptEnabled.value = promptConfig.enabled ?? true
      defaultAppendMode.value = promptConfig.default_append_mode ?? false // 加载默认追加模式配置
      console.log('PopupInput: 加载到的prompt数量:', customPrompts.value.length)
      console.log('PopupInput: 默认追加模式:', defaultAppendMode.value)
      console.log('PopupInput: 条件性prompt列表:', customPrompts.value.filter(p => p.type === 'conditional'))

      // 同步到拖拽列表（只包含普通prompt）
      sortablePrompts.value = [...normalPrompts.value]
      console.log('PopupInput: 同步到sortablePrompts:', sortablePrompts.value.length)

      // 延迟初始化拖拽功能，等待组件完全挂载
      if (customPrompts.value.length > 0) {
        console.log('PopupInput: 准备启动拖拽功能')
        initializeDragSort()
      }
      else {
        console.log('PopupInput: 没有prompt，跳过拖拽初始化')
      }
    }
  }
  catch (error) {
    console.error('PopupInput: 加载自定义prompt失败:', error)
  }
}

// 处理自定义prompt点击
function handlePromptClick(prompt: CustomPrompt) {
  // 如果prompt内容为空或只有空格，直接清空输入框
  if (!prompt.content || prompt.content.trim() === '') {
    setEditorPlainText('')
    emitUpdate()
    return
  }

  // 从 textarea 获取最新值来判断是否有内容
  const currentValue = getCurrentInputValue()
  const hasUserInput = currentValue.trim().length > 0

  if (hasUserInput) {
    // 如果输入框有内容，始终追加（保护用户已输入的内容）
    insertPromptContent(prompt.content, 'append')
  }
  else {
    // 如果输入框为空，检查默认追加模式
    if (defaultAppendMode.value) {
      // 默认追加模式：追加
      insertPromptContent(prompt.content, 'append')
    }
    else {
      // 非默认追加模式：直接替换（因为输入框是空的）
      insertPromptContent(prompt.content, 'replace')
    }
  }
}

// 处理引用消息内容
function handleQuoteMessage(messageContent: string) {
  if (userInput.value.trim()) {
    // 输入框有内容，显示插入选择对话框
    pendingPromptContent.value = messageContent
    showInsertDialog.value = true
  }
  else {
    // 输入框为空，直接插入
    insertPromptContent(messageContent)
    message.success('原文内容已引用到输入框')
  }
}

// 插入prompt内容
async function insertPromptContent(content: string, mode: 'replace' | 'append' = 'replace') {
  // 先从编辑器获取最新值（避免防抖延迟）
  const currentValue = getCurrentInputValue()

  console.log('[DEBUG] insertPromptContent 开始:', { content, mode, currentValue })

  if (mode === 'replace') {
    setEditorPlainText(content)
  }
  else {
    moveCaretToEnd()
    insertTextAtCaret(`${currentValue.trim() ? '\n\n' : ''}${content}`)
    userInput.value = getCurrentInputValue()
  }

  console.log('[DEBUG] insertPromptContent 设置后:', { userInput: userInput.value })

  // 等待 Vue 更新 DOM，确保编辑器的值已同步
  await nextTick()

  // 聚焦到输入框
  setTimeout(() => {
    if (textareaRef.value) {
      focusEditor()
      moveCaretToEnd()
    }
  }, 100)

  console.log('[DEBUG] insertPromptContent 调用 emitUpdateImmediate')
  emitUpdateImmediate() // prompt插入需要即时响应
}

// 处理插入模式选择
function handleInsertMode(mode: 'replace' | 'append') {
  insertPromptContent(pendingPromptContent.value, mode)
  showInsertDialog.value = false
  pendingPromptContent.value = ''
}

// 处理条件性prompt开关变化
async function handleConditionalToggle(promptId: string, value: boolean) {
  // 先更新本地状态
  const prompt = customPrompts.value.find(p => p.id === promptId)
  if (prompt) {
    prompt.current_state = value
  }

  // 保存到后端
  try {
    await invoke('update_conditional_prompt_state', {
      promptId,
      newState: value,
    })
    message.success('上下文追加状态已保存')
  }
  catch (error) {
    console.error('保存条件性prompt状态失败:', error)
    const errorMessage = error instanceof Error ? error.message : String(error)
    message.error(`保存设置失败: ${errorMessage}`)

    // 回滚本地状态
    if (prompt) {
      prompt.current_state = !value
    }
  }
}

// 生成条件性prompt的追加内容
function generateConditionalContent(): string {
  const conditionalTexts: string[] = []

  conditionalPrompts.value.forEach((prompt) => {
    const isEnabled = prompt.current_state ?? false
    const template = isEnabled ? prompt.template_true : prompt.template_false

    if (template && template.trim()) {
      conditionalTexts.push(template.trim())
    }
  })

  return conditionalTexts.length > 0 ? `\n\n${conditionalTexts.join('\n')}` : ''
}

// 获取条件性prompt的自适应描述
function getConditionalDescription(prompt: CustomPrompt): string {
  const isEnabled = prompt.current_state ?? false
  const template = isEnabled ? prompt.template_true : prompt.template_false

  // 如果有对应状态的模板，显示模板内容，否则显示原始描述
  if (template && template.trim()) {
    return template.trim()
  }

  return prompt.description || ''
}

// 移除拖拽排序初始化函数

// 初始化拖拽排序功能
async function initializeDragSort() {
  console.log('PopupInput: initializeDragSort 被调用')

  // 等待多个tick确保DOM完全渲染
  await nextTick()
  await nextTick()

  // 使用更长的延迟
  setTimeout(async () => {
    console.log('PopupInput: 开始查找容器')

    // 尝试多种方式查找容器
    let targetContainer = promptContainer.value

    if (!targetContainer) {
      targetContainer = document.querySelector('[data-prompt-container]') as HTMLElement
      console.log('PopupInput: querySelector结果:', targetContainer)
    }

    if (!targetContainer) {
      // 尝试通过类名查找
      const containers = document.querySelectorAll('.flex.flex-wrap')
      console.log('PopupInput: 找到的flex容器数量:', containers.length)
      for (let i = 0; i < containers.length; i++) {
        const container = containers[i] as HTMLElement
        if (container.querySelector('.sortable-item')) {
          targetContainer = container
          console.log('PopupInput: 通过sortable-item找到容器')
          break
        }
      }
    }

    if (targetContainer) {
      console.log('PopupInput: 找到目标容器:', targetContainer)
      const dragHandles = targetContainer.querySelectorAll('.drag-handle')
      console.log('PopupInput: 找到拖拽手柄数量:', dragHandles.length)

      const sortableItems = targetContainer.querySelectorAll('.sortable-item')
      console.log('PopupInput: 找到可排序项数量:', sortableItems.length)

      // 更新容器引用
      promptContainer.value = targetContainer

      console.log('PopupInput: 调用start()函数')
      start()
      console.log('PopupInput: start()函数调用完成')
    }
    else {
      console.log('PopupInput: 无法找到容器，DOM可能还没有渲染')
      console.log('PopupInput: 当前页面所有带data-prompt-container的元素:', document.querySelectorAll('[data-prompt-container]'))
      console.log('PopupInput: 当前页面所有.sortable-item元素:', document.querySelectorAll('.sortable-item'))
    }
  }, 500) // 增加延迟时间
}

// 保存prompt排序
async function savePromptOrder() {
  try {
    console.log('savePromptOrder被调用')
    console.log('当前sortablePrompts:', sortablePrompts.value.map(p => ({ id: p.id, name: p.name })))
    const promptIds = sortablePrompts.value.map(p => p.id)
    console.log('开始保存排序，prompt IDs:', promptIds)

    const startTime = Date.now()
    await invoke('update_custom_prompt_order', { promptIds })
    const endTime = Date.now()

    console.log(`排序已保存，耗时: ${endTime - startTime}ms`)
    message.success('排序已保存')
  }
  catch (error) {
    console.error('保存排序失败:', error)
    message.error('保存排序失败')
    // 重新加载以恢复原始顺序
    loadCustomPrompts()
  }
}

// 监听用户输入变化（仅用于非原生输入场景）
// watch(userInput, () => {
//   emitUpdate()
// })

// 输入法组合状态
const isComposing = ref(false)
const debouncedParseInlineReferences = useDebounceFn(() => {
  void scanEditorForInlineReferences()
}, 180)

function serializeEditorNode(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) {
    return node.textContent || ''
  }

  if (!(node instanceof HTMLElement)) {
    return ''
  }

  if (node.dataset.referenceLabel) {
    return node.dataset.referenceSerialized || ''
  }

  if (node.tagName === 'BR') {
    return '\n'
  }

  const content = Array.from(node.childNodes).map(serializeEditorNode).join('')

  if (node === textareaRef.value) {
    return content
  }

  if (node.tagName === 'DIV' || node.tagName === 'P') {
    return `${content}\n`
  }

  return content
}

// 获取当前输入框的实际值
function getCurrentInputValue(): string {
  if (textareaRef.value) {
    return serializeEditorNode(textareaRef.value).replace(/\u00A0/g, ' ').replace(/\n{3,}/g, '\n\n').trimEnd()
  }
  return userInput.value
}

// 处理富文本输入
function handleTextInput(event: Event) {
  // 输入法组合期间不更新
  if (isComposing.value)
    return

  syncReferencedFilesWithEditor()

  const editor = event.target as HTMLDivElement
  if (editor) {
    userInput.value = getCurrentInputValue()
  }

  // 触发防抖的 emit（避免频繁通知父组件）
  debouncedEmitUpdate()
  debouncedParseInlineReferences()
}

// 输入法开始组合
function handleCompositionStart() {
  isComposing.value = true
}

// 输入法结束组合
function handleCompositionEnd(event: Event) {
  isComposing.value = false

  syncReferencedFilesWithEditor()

  const editor = event.target as HTMLDivElement
  if (editor) {
    userInput.value = getCurrentInputValue()
  }

  debouncedEmitUpdate()
  debouncedParseInlineReferences()
}

// 自动调整 textarea 高度 - 简化版，避免滚动问题
const debouncedResize = useDebounceFn(() => {
  // 固定高度，依赖原生滚动
  // 不再动态调整高度，避免滚动位置错乱
}, 100)

// 处理粘贴后调整高度
function handlePasteResize() {
  setTimeout(() => debouncedResize(), 50)
}

// 移除拖拽相关的监听器

// 事件监听器引用
let unlistenCustomPromptUpdate: (() => void) | null = null
let unlistenWindowMove: (() => void) | null = null
let unlistenDragDrop: (() => void) | null = null

// 修复输入法候选框位置的函数
function fixIMEPosition() {
  if (textareaRef.value) {
    try {
      const inputElement = textareaRef.value

      if (inputElement && document.activeElement === inputElement) {
        // 先失焦再聚焦，让输入法重新计算位置
        inputElement.blur()
        setTimeout(() => {
          inputElement.focus()
        }, 10)
      }
    }
    catch (error) {
      console.debug('修复IME位置失败:', error)
    }
  }
}

// 设置窗口移动监听器
async function setupWindowMoveListener() {
  try {
    const webview = getCurrentWebviewWindow()

    // 监听窗口移动事件
    unlistenWindowMove = await webview.onMoved(() => {
      // 窗口移动后修复输入法位置
      fixIMEPosition()
    })

    console.log('窗口移动监听器已设置')
  }
  catch (error) {
    console.error('设置窗口移动监听器失败:', error)
  }
}

async function setupDragDropListener() {
  try {
    const webview = getCurrentWebviewWindow()

    unlistenDragDrop = await webview.onDragDropEvent(async (event) => {
      switch (event.payload.type) {
        case 'enter':
        case 'over':
          isDragOver.value = true
          break
        case 'drop':
          isDragOver.value = false
          await handleDroppedPaths(event.payload.paths)
          break
        case 'leave':
          isDragOver.value = false
          break
      }
    })

    console.log('文件拖放监听器已设置')
  }
  catch (error) {
    console.error('设置文件拖放监听器失败:', error)
  }
}

// 组件挂载时加载自定义prompt
onMounted(async () => {
  console.log('组件挂载，开始加载prompt')
  await loadCustomPrompts()

  // 监听自定义prompt更新事件
  unlistenCustomPromptUpdate = await listen('custom-prompt-updated', () => {
    console.log('收到自定义prompt更新事件，重新加载数据')
    loadCustomPrompts()
  })

  // 设置窗口移动监听器
  setupWindowMoveListener()
  setupDragDropListener()
})

onUnmounted(() => {
  // 清理事件监听器
  if (unlistenCustomPromptUpdate) {
    unlistenCustomPromptUpdate()
  }

  // 清理窗口移动监听器
  if (unlistenWindowMove) {
    unlistenWindowMove()
  }

  if (unlistenDragDrop) {
    unlistenDragDrop()
  }

  // 停止拖拽功能
  stop()
})

watch(() => props.request?.id, (newRequestId, oldRequestId) => {
  if (newRequestId && newRequestId !== oldRequestId) {
    reset()
  }
})

// 重置数据
function reset() {
  setEditorPlainText('')
  selectedOptions.value = []
  uploadedImages.value = []
  referencedFiles.value = []
  isDragOver.value = false
  emitUpdate()
}

// 更新数据（用于外部同步）
function updateData(data: { userInput?: string, selectedOptions?: string[], draggedImages?: string[], referencedFiles?: FileReferenceAttachment[] }) {
  if (data.userInput !== undefined) {
    setEditorPlainText(data.userInput)
  }
  if (data.selectedOptions !== undefined) {
    selectedOptions.value = data.selectedOptions
  }
  if (data.draggedImages !== undefined) {
    uploadedImages.value = data.draggedImages
  }
  if (data.referencedFiles !== undefined) {
    referencedFiles.value = data.referencedFiles
    nextTick(() => {
      for (const reference of referencedFiles.value) {
        insertReferenceBadge(reference)
      }
    })
  }

  emitUpdate()
}

// 移除了文件选择和测试图片功能

// 暴露方法给父组件
defineExpose({
  reset,
  canSubmit,
  statusText,
  updateData,
  handleQuoteMessage,
  getCurrentInputValue, // 暴露获取当前输入值的方法
})
</script>

<template>
  <div class="space-y-3">
    <!-- 预定义选项 -->
    <div v-if="!loading && hasOptions" class="space-y-3" data-guide="predefined-options">
      <h4 class="text-sm font-medium text-white">
        请选择选项
      </h4>
      <n-space vertical size="small">
        <div
          v-for="(option, index) in request!.predefined_options"
          :key="`option-${index}`"
          class="rounded-lg p-3 border border-gray-600 bg-gray-100 cursor-pointer hover:opacity-80 transition-opacity"
          @click="handleOptionToggle(option)"
        >
          <n-checkbox
            :value="option"
            :checked="selectedOptions.includes(option)"
            :disabled="submitting"
            size="medium"
            @update:checked="(checked: boolean) => handleOptionChange(option, checked)"
            @click.stop
          >
            {{ option }}
          </n-checkbox>
        </div>
      </n-space>
    </div>

    <!-- 图片预览区域 -->
    <div v-if="!loading && uploadedImages.length > 0" class="space-y-3">
      <h4 class="text-sm font-medium text-white">
        已添加的图片 ({{ uploadedImages.length }})
      </h4>

      <!-- 使用 Naive UI 的图片组件，支持预览和放大 -->
      <n-image-group>
        <div class="flex flex-wrap gap-3">
          <div
            v-for="(image, index) in uploadedImages"
            :key="`image-${index}`"
            class="relative"
          >
            <!-- 使用 n-image 组件，启用预览功能 -->
            <n-image
              :src="image"
              width="100"
              height="100"
              object-fit="cover"
              class="rounded-lg border-2 border-gray-300 hover:border-primary-400 transition-all duration-200 cursor-pointer"
            />

            <!-- 删除按钮 -->
            <n-button
              class="absolute -top-2 -right-2 z-10"
              size="tiny"
              type="error"
              circle
              @click="removeImage(index)"
            >
              <template #icon>
                <div class="i-carbon-close w-3 h-3" />
              </template>
            </n-button>

            <!-- 序号 -->
            <div class="absolute bottom-1 left-1 w-5 h-5 bg-primary-500 text-white text-xs rounded-full flex items-center justify-center font-bold shadow-sm z-5">
              {{ index + 1 }}
            </div>
          </div>
        </div>
      </n-image-group>
    </div>

    <!-- 文本输入区域 -->
    <div v-if="!loading" class="space-y-3">
      <h4 class="text-sm font-medium text-white">
        {{ hasOptions ? '补充说明 (可选)' : '请输入您的回复' }}
      </h4>

      <!-- 自定义prompt按钮区域 -->
      <div v-if="customPromptEnabled && customPrompts.length > 0" class="space-y-2" data-guide="custom-prompts">
        <div class="text-xs text-on-surface-secondary flex items-center gap-2">
          <div class="i-carbon-bookmark w-3 h-3 text-primary-500" />
          <span>快捷模板 (拖拽调整顺序):</span>
        </div>
        <div
          ref="promptContainer"
          data-prompt-container
          class="flex flex-wrap gap-2"
        >
          <div
            v-for="prompt in sortablePrompts"
            :key="prompt.id"
            :title="prompt.description || (prompt.content.trim() ? prompt.content : '清空输入框')"
            class="inline-flex items-center gap-1 px-2 py-1 text-xs bg-container-secondary hover:bg-container-tertiary rounded transition-all duration-200 select-none border border-gray-600 text-on-surface sortable-item"
          >
            <!-- 拖拽手柄 -->
            <div class="drag-handle cursor-move p-0.5 rounded hover:bg-container-tertiary transition-colors">
              <div class="i-carbon-drag-horizontal w-3 h-3 text-on-surface-secondary" />
            </div>

            <!-- 按钮内容 -->
            <div
              class="inline-flex items-center cursor-pointer"
              @click="handlePromptClick(prompt)"
            >
              <span>{{ prompt.name }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 上下文追加区域 -->
      <div v-if="customPromptEnabled && conditionalPrompts.length > 0" class="space-y-2" data-guide="context-append">
        <div class="text-xs text-on-surface-secondary flex items-center gap-2">
          <div class="i-carbon-settings-adjust w-3 h-3 text-primary-500" />
          <span>上下文追加:</span>
        </div>
        <div class="grid grid-cols-2 gap-2">
          <div
            v-for="prompt in conditionalPrompts"
            :key="prompt.id"
            class="flex items-center justify-between p-2 bg-container-secondary rounded border border-gray-600 hover:bg-container-tertiary transition-colors text-xs"
          >
            <div class="flex-1 min-w-0 mr-2">
              <div class="text-xs text-on-surface truncate font-medium" :title="prompt.condition_text || prompt.name">
                {{ prompt.condition_text || prompt.name }}
              </div>
              <div v-if="getConditionalDescription(prompt)" class="text-xs text-primary-600 dark:text-primary-400 opacity-50 dark:opacity-60 mt-0.5 truncate leading-tight" :title="getConditionalDescription(prompt)">
                {{ getConditionalDescription(prompt) }}
              </div>
            </div>
            <n-switch
              :value="prompt.current_state ?? false"
              size="small"
              @update:value="(value: boolean) => handleConditionalToggle(prompt.id, value)"
            />
          </div>
        </div>
      </div>

      <!-- 附件提示区域 -->
      <div v-if="uploadedImages.length === 0 && referencedFiles.length === 0" class="text-center">
        <div class="text-xs text-on-surface-secondary">
          💡 提示：可以在输入框中粘贴图片 ({{ pasteShortcut }})，直接拖拽图片 / 文件 / 文件夹到窗口中添加引用，或粘贴绝对路径 / URL 自动转为引用 badge
        </div>
      </div>

      <div class="popup-input-shell" :class="{ 'popup-input-shell-dragover': isDragOver }">
        <div
          ref="textareaRef"
          class="popup-textarea"
          :contenteditable="!submitting"
          :data-placeholder="hasOptions ? `您可以在这里添加补充说明... (支持粘贴图片 ${pasteShortcut}、拖拽文件/文件夹、粘贴绝对路径或 URL)` : `请输入您的回复... (支持粘贴图片 ${pasteShortcut}、拖拽文件/文件夹、粘贴绝对路径或 URL)`"
          data-guide="popup-input"
          @keydown="handleEditorKeydown"
          @paste="handleImagePaste"
          @input="handleTextInput"
          @compositionstart="handleCompositionStart"
          @compositionend="handleCompositionEnd"
        ></div>
      </div>
    </div>

    <!-- 插入模式选择对话框 -->
    <n-modal v-model:show="showInsertDialog" preset="dialog" title="插入模式选择">
      <template #header>
        <div class="flex items-center gap-2">
          <div class="i-carbon-text-creation w-4 h-4" />
          <span>插入Prompt</span>
        </div>
      </template>
      <div class="space-y-4">
        <p class="text-sm text-on-surface-secondary">
          输入框中已有内容，请选择插入模式：
        </p>
        <div class="bg-container-secondary p-3 rounded text-sm">
          {{ pendingPromptContent }}
        </div>
      </div>
      <template #action>
        <div class="flex gap-2">
          <n-button @click="showInsertDialog = false">
            取消
          </n-button>
          <n-button type="warning" @click="handleInsertMode('replace')">
            替换内容
          </n-button>
          <n-button type="primary" @click="handleInsertMode('append')">
            追加内容
          </n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>

<style scoped>
/* Sortable.js 拖拽样式 */
.sortable-ghost {
  opacity: 0.5;
  transform: scale(0.95);
}

.sortable-chosen {
  cursor: grabbing !important;
}

.sortable-drag {
  opacity: 0.8;
  transform: rotate(5deg);
}

/* 原生 textarea 样式 - 适配主题 */
.popup-textarea {
  width: 100%;
  height: 120px; /* 固定高度，约5行 */
  padding: 0.5rem 0.75rem;
  font-size: 0.875rem;
  line-height: 1.5;
  border-radius: 0.75rem;
  resize: none;
  overflow-y: auto;
  overflow-x: hidden;
  transition: border-color 0.2s, box-shadow 0.2s;
  background-color: var(--color-surface-100, #f0f0f0);
  color: var(--color-on-surface, #333333);
  border: none;
  box-sizing: border-box;
  white-space: pre-wrap;
  word-break: break-word;
}

.popup-input-shell {
  border: 1px solid var(--color-surface-300, #d0d0d0);
  border-radius: 0.75rem;
  overflow: hidden;
  background-color: var(--color-surface-100, #f0f0f0);
  transition: border-color 0.2s, box-shadow 0.2s;
}

.popup-input-shell:focus-within {
  border-color: #14b8a6;
  box-shadow: 0 0 0 1px #14b8a6;
}

.popup-input-shell-dragover {
  border-color: #14b8a6;
  box-shadow: 0 0 0 1px #14b8a6;
}

.popup-textarea[contenteditable="true"]:empty::before {
  content: attr(data-placeholder);
  color: var(--color-surface-500, #808080);
  pointer-events: none;
}

.popup-textarea:focus {
  outline: none;
}

.popup-textarea[contenteditable="false"] {
  opacity: 0.6;
  cursor: not-allowed;
}

:deep(.popup-inline-reference) {
  display: inline-flex;
  max-width: 100%;
  align-items: center;
  gap: 0.3rem;
  margin: 0 0.18rem;
  padding: 0.12rem 0.42rem;
  border: 1px solid rgba(75, 85, 99, 0.9);
  border-radius: 9999px;
  background: rgba(31, 41, 55, 0.82);
  color: #f3f4f6;
  vertical-align: baseline;
  user-select: none;
  cursor: default;
  pointer-events: none;
}

:deep(.popup-inline-reference-kind) {
  border-radius: 9999px;
  background: rgba(75, 85, 99, 0.72);
  padding: 0.04rem 0.32rem;
  font-size: 10px;
  color: #d1d5db;
}

:deep(.popup-inline-reference-label) {
  max-width: 14rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
</style>
