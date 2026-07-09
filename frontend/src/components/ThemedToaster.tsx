'use client'

import { useTheme } from 'next-themes'
import { Toaster } from 'sonner'

/**
 * sonner Toaster that follows the app theme so toasts match light/dark.
 */
export function ThemedToaster() {
  const { resolvedTheme } = useTheme()

  return (
    <Toaster
      position="bottom-center"
      richColors
      closeButton
      theme={resolvedTheme === 'dark' ? 'dark' : 'light'}
    />
  )
}
