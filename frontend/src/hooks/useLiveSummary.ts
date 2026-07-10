'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useConfig } from '@/contexts/ConfigContext';

const REGEN_INTERVAL_MS = 60_000; // steady cadence between successful summaries
const FAST_RETRY_MS = 10_000; // retry cadence until the first summary lands
const FIRST_ATTEMPT_MS = 10_000; // let some transcript accrue before the first try
const MIN_CHARS_TO_SUMMARIZE = 200; // skip until there's enough to summarize

// Shared with useRecordingStop, which reads this after the meeting is saved to
// pre-fill the meeting's summary. Must NOT be cleared on stop (see the effect below).
export const LIVE_SUMMARY_STORAGE_KEY = 'last_live_summary_markdown';

// Matches the Rust sentinel returned when the configured summary model is not local.
const NO_LOCAL_MODEL_SENTINEL = 'LIVE_SUMMARY_LOCAL_ONLY';

// The live summary intentionally runs on-device only (Ollama or built-in AI).
const LOCAL_PROVIDERS = new Set(['ollama', 'builtin-ai']);

export interface LiveSummaryState {
  summaryMarkdown: string;
  isGenerating: boolean;
  lastUpdatedAt: number | null;
  error: string | null;
  needsLocalModel: boolean;
}

/**
 * Drives the ephemeral "live rolling summary" during an active recording:
 * every ~60s it summarizes the transcript-so-far (from `transcriptsRef`) into a
 * bullet list via the local-only `generate_live_summary` command.
 *
 * The latest summary is mirrored to sessionStorage so `useRecordingStop` can
 * pre-fill the saved meeting's summary with it.
 */
export function useLiveSummary(): LiveSummaryState {
  const { transcriptsRef } = useTranscripts();
  const { isRecording } = useRecordingState();
  const { modelConfig } = useConfig();

  const isLocalProvider =
    !!modelConfig?.provider && LOCAL_PROVIDERS.has(modelConfig.provider);

  const [summaryMarkdown, setSummaryMarkdown] = useState('');
  const [isGenerating, setIsGenerating] = useState(false);
  const [lastUpdatedAt, setLastUpdatedAt] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [needsLocalModel, setNeedsLocalModel] = useState(false);

  // Non-rendering guards:
  const lastSummarizedLenRef = useRef(0); // skip when the transcript hasn't grown (silence/pause)
  const producedFirstRef = useRef(false); // governs fast-retry vs steady cadence
  const wasRecordingRef = useRef(false); // detects the actual start transition

  const runOnce = useCallback(async () => {
    const text = transcriptsRef.current
      .map((t) => t.text)
      .join(' ')
      .trim();

    if (text.length < MIN_CHARS_TO_SUMMARIZE) return; // too little to summarize yet
    if (text.length === lastSummarizedLenRef.current) return; // unchanged since last run

    setIsGenerating(true);
    try {
      const md =
        (await invoke<string>('generate_live_summary', { transcript: text }))?.trim() ?? '';
      lastSummarizedLenRef.current = text.length;
      if (md) {
        setSummaryMarkdown(md);
        producedFirstRef.current = true;
        try {
          sessionStorage.setItem(LIVE_SUMMARY_STORAGE_KEY, md);
        } catch {
          /* sessionStorage unavailable — pre-fill simply won't happen */
        }
      }
      setLastUpdatedAt(Date.now());
      setNeedsLocalModel(false);
      setError(null);
    } catch (e) {
      const msg = typeof e === 'string' ? e : (e as Error)?.message ?? String(e);
      if (msg.includes(NO_LOCAL_MODEL_SENTINEL)) {
        setNeedsLocalModel(true);
        setError(null);
      } else {
        setError(msg);
      }
      // Do NOT advance lastSummarizedLenRef on failure so the next tick retries.
    } finally {
      setIsGenerating(false);
    }
  }, [transcriptsRef]);

  // Keep the latest runOnce in a ref so the scheduler effect depends only on
  // isRecording / isLocalProvider (not on every transcript change).
  const runOnceRef = useRef(runOnce);
  useEffect(() => {
    runOnceRef.current = runOnce;
  }, [runOnce]);

  useEffect(() => {
    const justStarted = isRecording && !wasRecordingRef.current;
    wasRecordingRef.current = isRecording;

    // Reset in-memory state on any recording start/stop transition.
    setSummaryMarkdown('');
    setIsGenerating(false);
    setLastUpdatedAt(null);
    setError(null);
    lastSummarizedLenRef.current = 0;
    producedFirstRef.current = false;

    if (!isRecording) {
      setNeedsLocalModel(false);
      // Intentionally do NOT clear LIVE_SUMMARY_STORAGE_KEY here: useRecordingStop
      // reads it after the async save completes (seconds after isRecording flips),
      // and removes it once consumed.
      return;
    }

    // Only on a genuine start transition, drop any stale summary from a previous
    // meeting. Guarded so an incidental effect re-run mid-recording (e.g. the model
    // provider changing) can't wipe the in-progress summary that pre-fill relies on.
    if (justStarted) {
      try {
        sessionStorage.removeItem(LIVE_SUMMARY_STORAGE_KEY);
      } catch {
        /* ignore */
      }
    }

    // Local-only: show the hint immediately and make no calls for cloud/no model.
    if (!isLocalProvider) {
      setNeedsLocalModel(true);
      return;
    }
    setNeedsLocalModel(false);

    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    const tick = async () => {
      await runOnceRef.current(); // awaited → the next tick is scheduled only after this returns (no overlap)
      if (cancelled) return;
      const delay = producedFirstRef.current ? REGEN_INTERVAL_MS : FAST_RETRY_MS;
      timer = setTimeout(tick, delay);
    };
    timer = setTimeout(tick, FIRST_ATTEMPT_MS);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [isRecording, isLocalProvider]);

  return { summaryMarkdown, isGenerating, lastUpdatedAt, error, needsLocalModel };
}
