import { useState, useEffect, useRef, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

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
 * Loads a meeting's recorded audio into an HTMLAudioElement via a Blob URL and
 * exposes native play/pause/seek. `audioPath` is an absolute file path
 * (e.g. `${folder_path}/audio.mp4`) or null.
 *
 * A read failure (missing file — e.g. auto-save was off, or an imported non-mp4
 * recording) degrades gracefully to `hasAudio = false` rather than surfacing an error.
 */
export function useMeetingAudioPlayer(audioPath: string | null): MeetingAudioPlayer {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const objectUrlRef = useRef<string | null>(null);

  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [hasAudio, setHasAudio] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Reset for the new path.
    setIsPlaying(false);
    setCurrentTime(0);
    setDuration(0);
    setHasAudio(false);
    setError(null);

    if (!audioPath) {
      setIsLoading(false);
      return;
    }

    let cancelled = false;
    const audio = new Audio();
    audio.preload = 'metadata';
    audioRef.current = audio;

    const onLoadedMetadata = () => {
      if (Number.isFinite(audio.duration)) setDuration(audio.duration);
      setHasAudio(true);
    };
    const onTimeUpdate = () => setCurrentTime(audio.currentTime);
    const onPlay = () => setIsPlaying(true);
    const onPause = () => setIsPlaying(false);
    const onEnded = () => {
      setIsPlaying(false);
      setCurrentTime(Number.isFinite(audio.duration) ? audio.duration : 0);
    };
    const onError = () => {
      if (cancelled) return;
      setHasAudio(false);
      setError('Unable to play this audio file');
    };

    audio.addEventListener('loadedmetadata', onLoadedMetadata);
    audio.addEventListener('timeupdate', onTimeUpdate);
    audio.addEventListener('play', onPlay);
    audio.addEventListener('pause', onPause);
    audio.addEventListener('ended', onEnded);
    audio.addEventListener('error', onError);

    setIsLoading(true);
    (async () => {
      try {
        // read_audio_file returns raw bytes (tauri::ipc::Response) → ArrayBuffer here.
        const buf = await invoke<ArrayBuffer>('read_audio_file', { filePath: audioPath });
        if (cancelled) return;
        if (!buf || buf.byteLength === 0) throw new Error('Empty audio data');

        const blob = new Blob([buf], { type: 'audio/mp4' });
        const url = URL.createObjectURL(blob);
        objectUrlRef.current = url;
        audio.src = url; // hasAudio flips true on 'loadedmetadata'
      } catch (e) {
        if (cancelled) return;
        // Missing file (no checkpoints) or non-mp4 import → treat as "no audio".
        console.warn('No playable audio for meeting:', e);
        setHasAudio(false);
        setError(null);
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    })();

    return () => {
      cancelled = true;
      audio.pause();
      audio.removeEventListener('loadedmetadata', onLoadedMetadata);
      audio.removeEventListener('timeupdate', onTimeUpdate);
      audio.removeEventListener('play', onPlay);
      audio.removeEventListener('pause', onPause);
      audio.removeEventListener('ended', onEnded);
      audio.removeEventListener('error', onError);
      audio.removeAttribute('src');
      audio.load(); // release the source
      if (objectUrlRef.current) {
        URL.revokeObjectURL(objectUrlRef.current);
        objectUrlRef.current = null;
      }
      audioRef.current = null;
    };
  }, [audioPath]);

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
