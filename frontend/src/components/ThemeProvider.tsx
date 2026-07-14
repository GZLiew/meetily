'use client'

import { ThemeProvider as NextThemesProvider } from 'next-themes'
import type { ComponentProps } from 'react'

/**
 * App-wide theme provider (light / dark / system).
 *
 * Uses the `class` strategy so the `.dark` class is toggled on <html>, which
 * flips the shadcn CSS variables defined in globals.css. Defaults to dark; users
 * can still switch to light/system via the ThemeToggle (persisted by next-themes).
 */
export function ThemeProvider({ children, ...props }: ComponentProps<typeof NextThemesProvider>) {
  return (
    <NextThemesProvider
      attribute="class"
      defaultTheme="dark"
      enableSystem
      disableTransitionOnChange
      {...props}
    >
      {children}
    </NextThemesProvider>
  )
}
