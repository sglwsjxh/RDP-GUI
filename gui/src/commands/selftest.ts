// SPDX-License-Identifier: AGPL-3.0-only
import { invoke } from '@tauri-apps/api/core'

export interface SelftestResult {
  passed: boolean
  tests: Array<{
    name: string
    passed: boolean
    message?: string
  }>
}

export async function runSelftest(): Promise<SelftestResult> {
  return invoke('run_selftest')
}
