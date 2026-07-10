'use client';

import { useEffect, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Loader2, Sparkles, Settings2 } from 'lucide-react';
import { useLiveSummary } from '@/hooks/useLiveSummary';

// Re-renders a relative "updated Ns ago" label off a fixed timestamp.
function useAgoLabel(ts: number | null): string {
  const [, force] = useState(0);
  useEffect(() => {
    if (ts == null) return;
    const id = setInterval(() => force((n) => n + 1), 5_000);
    return () => clearInterval(id);
  }, [ts]);

  if (ts == null) return '';
  const secs = Math.max(0, Math.round((Date.now() - ts) / 1000));
  if (secs < 5) return 'just now';
  if (secs < 60) return `${secs}s ago`;
  return `${Math.round(secs / 60)}m ago`;
}

/**
 * Live rolling summary shown beside the transcript during an active recording.
 * Renders the bullet markdown produced by `useLiveSummary` (regenerated ~every
 * 60s from the transcript so far). Read-only — deliberately not the BlockNote editor.
 */
export function LiveSummaryPanel() {
  const { summaryMarkdown, isGenerating, lastUpdatedAt, error, needsLocalModel } = useLiveSummary();
  const ago = useAgoLabel(lastUpdatedAt);

  return (
    <div className="hidden md:flex flex-1 min-w-0 bg-card flex-col overflow-hidden">
      {/* Header */}
      <div className="sticky top-0 z-10 flex items-center justify-between gap-2 bg-card p-4 border-b border-border">
        <div className="flex items-center gap-2">
          <Sparkles size={16} className="text-muted-foreground" />
          <h2 className="text-sm font-semibold">Live Summary</h2>
        </div>
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          {isGenerating ? (
            <Loader2 size={14} className="animate-spin" aria-label="Updating" />
          ) : lastUpdatedAt != null ? (
            <span>updated {ago}</span>
          ) : null}
        </div>
      </div>

      {/* Body */}
      <div className="flex-1 overflow-y-auto p-4">
        {needsLocalModel ? (
          <div className="flex flex-col items-center gap-2 pt-8 text-center text-muted-foreground">
            <Settings2 size={20} />
            <p className="text-sm">Live summary runs on a local model.</p>
            <p className="text-xs">
              Set Ollama or built-in AI as your model in Settings to see a summary while you record.
            </p>
          </div>
        ) : summaryMarkdown ? (
          <div className="prose prose-sm dark:prose-invert max-w-none [&_ul]:list-disc [&_ul]:pl-5 [&_li]:my-0.5">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{summaryMarkdown}</ReactMarkdown>
          </div>
        ) : error ? (
          <p className="text-xs text-muted-foreground">Couldn&apos;t update the summary just now. Retrying…</p>
        ) : (
          <p className="text-sm text-muted-foreground">Listening… a summary will appear shortly.</p>
        )}
      </div>
    </div>
  );
}
