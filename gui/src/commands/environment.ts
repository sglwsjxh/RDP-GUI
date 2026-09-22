// SPDX-License-Identifier: AGPL-3.0-only
import { invoke } from '@tauri-apps/api/core'

export interface EnvCheckResult {
  os: string
  arch: string
  rustVersion: string
  tauriVersion: string
  gpu: string
  memory: string
  checks: Record<string, boolean>
}

export async function checkEnvironment(): Promise<EnvCheckResult> {
  return invoke('cmd_env_check')
}

export async function fixEnvironment(): Promise<void> {
  return invoke('cmd_apply_fixes')
}
