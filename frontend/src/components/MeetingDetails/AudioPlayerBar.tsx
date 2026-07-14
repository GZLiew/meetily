'use client';

import { Play, Pause } from 'lucide-react';

function formatTime(t: number): string {
  if (!Number.isFinite(t) || t < 0) t = 0;
  const m = Math.floor(t / 60);
  const s = Math.floor(t % 60);
  return `${m}:${s.toString().padStart(2, '0')}`;
}

interface AudioPlayerBarProps {
  isPlaying: boolean;
  currentTime: number;
  duration: number;
  onToggle: () => void;
  onSeek: (seconds: number) => void;
}

/**
 * Compact audio player: play/pause + a seekable progress bar + M:SS / M:SS time.
 * Rendered only when the meeting has playable audio.
 */
export function AudioPlayerBar({ isPlaying, currentTime, duration, onToggle, onSeek }: AudioPlayerBarProps) {
  const max = duration > 0 ? duration : 0;
  const value = Math.min(currentTime, max);

  return (
    <div className="mt-3 flex items-center gap-2 rounded-md border border-border bg-card px-2 py-1.5">
      <button
        type="button"
        onClick={onToggle}
        aria-label={isPlaying ? 'Pause' : 'Play'}
        className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-muted text-foreground transition-colors hover:bg-accent"
      >
        {isPlaying ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4 translate-x-[1px]" />}
      </button>

      <input
        type="range"
        min={0}
        max={max}
        step={0.1}
        value={value}
        onChange={(e) => onSeek(parseFloat(e.target.value))}
        aria-label="Seek audio"
        className="h-1 flex-1 cursor-pointer appearance-none rounded-full bg-muted accent-blue-500"
      />

      <span className="min-w-[76px] shrink-0 text-right text-xs tabular-nums text-muted-foreground">
        {formatTime(currentTime)} / {formatTime(duration)}
      </span>
    </div>
  );
}
