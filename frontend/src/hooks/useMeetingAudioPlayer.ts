import { useState, useEffect, useRef, useCallback } from 'react';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

export interface MeetingAudioPlayer {
  isPlaying: boolean;
  currentTime: number;
  duration: number;
  hasAudio: boolean;
  isLoading: boolean;
  error: string | null;
  play: () => void;
  pause: () => void;
  toggle: () => void;
  seek: (seconds: number) => void;
}

/**
 * Plays a meeting's recorded audio in the meeting-details view.
 *
 * Given the meeting's recording `folderPath`, it resolves the actual audio file
 * (audio.mp4, or an imported .m4a/.wav/...) via the `resolve_meeting_audio_path`
 * command, then loads it into an HTMLAudioElement through Tauri's asset protocol
 * (`convertFileSrc`). The asset protocol streams with HTTP range support, so it
 * seeks reliably and — unlike a `blob:` URL — works in the packaged app, not just
 * `tauri dev`.
 *
 * When the meeting has no audio file (auto-save was off), `hasAudio` stays false
 * and no error is set, so the UI can simply omit the player.
 */
export function useMeetingAudioPlayer(folderPath: string | null): MeetingAudioPlayer {
  const audioRef = useRef<HTMLAudioElement | null>(null);

  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [hasAudio, setHasAudio] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Reset for the new meeting.
    setIsPlaying(false);
    setCurrentTime(0);
    setDuration(0);
    setHasAudio(false);
    setError(null);

    if (!folderPath) {
      setIsLoading(false);
      return;
    }

    let cancelled = false;
    let audioEl: HTMLAudioElement | null = null;

    const onLoadedMetadata = () => {
      const a = audioRef.current;
      if (a && Number.isFinite(a.duration)) setDuration(a.duration);
      setHasAudio(true);
    };
    const onTimeUpdate = () => {
      const a = audioRef.current;
      if (a) setCurrentTime(a.currentTime);
    };
    const onPlay = () => setIsPlaying(true);
    const onPause = () => setIsPlaying(false);
    const onEnded = () => {
      const a = audioRef.current;
      setIsPlaying(false);
      setCurrentTime(a && Number.isFinite(a.duration) ? a.duration : 0);
    };
    const onError = () => {
      if (cancelled) return;
      setHasAudio(false);
      setError('Unable to play this audio file');
    };

    const detach = () => {
      const a = audioEl;
      if (!a) return;
      a.pause();
      a.removeEventListener('loadedmetadata', onLoadedMetadata);
      a.removeEventListener('timeupdate', onTimeUpdate);
      a.removeEventListener('play', onPlay);
      a.removeEventListener('pause', onPause);
      a.removeEventListener('ended', onEnded);
      a.removeEventListener('error', onError);
      a.removeAttribute('src');
      a.load(); // release the source
      audioEl = null;
      audioRef.current = null;
    };

    setIsLoading(true);
    (async () => {
      let audioPath: string | null = null;
      try {
        audioPath = await invoke<string | null>('resolve_meeting_audio_path', { folderPath });
      } catch (e) {
        console.warn('Failed to resolve meeting audio path:', e);
      }
      if (cancelled) return;

      if (!audioPath) {
        // Meeting recorded without audio (auto-save off) — no player, no error.
        setIsLoading(false);
        return;
      }

      const audio = new Audio();
      audio.preload = 'metadata';
      audioEl = audio;
      audioRef.current = audio;

      audio.addEventListener('loadedmetadata', onLoadedMetadata);
      audio.addEventListener('timeupdate', onTimeUpdate);
      audio.addEventListener('play', onPlay);
      audio.addEventListener('pause', onPause);
      audio.addEventListener('ended', onEnded);
      audio.addEventListener('error', onError);

      // Asset protocol URL (streams + range-seeks; works in the packaged app).
      audio.src = convertFileSrc(audioPath);
      setIsLoading(false);
    })();

    return () => {
      cancelled = true;
      detach();
    };
  }, [folderPath]);

  const play = useCallback(() => {
    audioRef.current?.play().catch((err) => console.warn('audio play() rejected:', err));
  }, []);

  const pause = useCallback(() => {
    audioRef.current?.pause();
  }, []);

  const toggle = useCallback(() => {
    const a = audioRef.current;
    if (!a) return;
    if (a.paused) a.play().catch((err) => console.warn('audio play() rejected:', err));
    else a.pause();
  }, []);

  const seek = useCallback((seconds: number) => {
    const a = audioRef.current;
    if (!a) return;
    const max = Number.isFinite(a.duration) ? a.duration : seconds;
    const clamped = Math.max(0, Math.min(seconds, max));
    a.currentTime = clamped;
    setCurrentTime(clamped);
  }, []);

  return { isPlaying, currentTime, duration, hasAudio, isLoading, error, play, pause, toggle, seek };
}
