'use client'

import { ThemeProvider as NextThemesProvider } from 'next-themes'
import type { ComponentProps } from 'react'

/**
 * App-wide theme provider (light / dark / system).
 *
 * Uses the `class` strategy so the `.dark` class is toggled on <html>, which
 * flips the shadcn CSS variables defined in globals.css. `defaultTheme="light"`
 * preserves the historical light appearance for existing users until they opt in.
 */
export function ThemeProvider({ children, ...props }: ComponentProps<typeof NextThemesProvider>) {
  return (
    <NextThemesProvider
      attribute="class"
      defaultTheme="light"
      enableSystem
      disableTransitionOnChange
      {...props}
    >
      {children}
    </NextThemesProvider>
  )
}
