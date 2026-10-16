import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { beforeEach, afterEach, it, expect, vi } from 'vitest'
import { ModelSetup } from '../ModelSetup'

const bridge = vi.hoisted(() => ({ getModelSetupStatus: vi.fn(), installRecommendedModel: vi.fn(), pauseModelSetup: vi.fn() }))
vi.mock('../tauriBridge.ts', () => bridge)
let root: Root
let container: HTMLDivElement
const initial = { active: false, ready: false, phase: '', modelName: '', completedBytes: 0, totalBytes: 0, message: '', cachePath: '/shared/hub' }
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  vi.resetAllMocks()
  bridge.getModelSetupStatus.mockResolvedValue(initial)
  container = document.createElement('div')
  root = createRoot(container)
})
afterEach(async () => { await act(async () => root.unmount()); container.remove() })
it('offers installation on an empty machine without asking for a file or CLI', async () => {
  const ready = vi.fn()
  await act(async () => root.render(createElement(ModelSetup, { disabled: false, onReady: ready })))
  const button = container.querySelector('button')!
  expect(button.textContent).toBe('Set up local model')
  expect(button.disabled).toBe(false)
  expect(container.querySelector('input')).toBeNull()
  bridge.installRecommendedModel.mockImplementation(async () => {
    bridge.getModelSetupStatus.mockResolvedValue({ ...initial, ready: true, phase: 'ready' })
  })
  await act(async () => button.click())
  expect(bridge.installRecommendedModel).toHaveBeenCalledOnce()
  expect(ready).toHaveBeenCalledOnce()
  expect(container.textContent).toContain('Local model installed')
})
it('shows resumable errors and never claims a failed installation is ready', async () => {
  const ready = vi.fn()
  bridge.getModelSetupStatus.mockResolvedValue({ ...initial, phase: 'paused', message: 'Setup paused.' })
  await act(async () => root.render(createElement(ModelSetup, { disabled: false, onReady: ready })))
  expect(container.querySelector('button')!.textContent).toBe('Resume model setup')
  bridge.installRecommendedModel.mockRejectedValue({ message: 'Connection interrupted. Resume setup.' })
  await act(async () => container.querySelector('button')!.click())
  expect(container.querySelector('[role="alert"]')!.textContent).toContain('Connection interrupted')
  expect(ready).not.toHaveBeenCalled()
})
it('restores progress for an ongoing installation and routes pause to the native downloader', async () => {
  bridge.getModelSetupStatus.mockResolvedValue({ ...initial, active: true, phase: 'downloading', modelName: 'Gemma', completedBytes: 50, totalBytes: 100 })
  await act(async () => root.render(createElement(ModelSetup, { disabled: false, onReady: vi.fn() })))
  expect(container.querySelector('progress')!.value).toBe(50)
  expect(container.querySelector('button')!.textContent).toBe('Pause setup')
  bridge.pauseModelSetup.mockResolvedValue(undefined)
  await act(async () => container.querySelector('button')!.click())
  expect(bridge.pauseModelSetup).toHaveBeenCalledOnce()
})
