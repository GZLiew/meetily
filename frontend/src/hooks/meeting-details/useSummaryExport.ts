import { useCallback, RefObject } from 'react';
import { Summary } from '@/types';
import { BlockNoteSummaryViewRef } from '@/components/AISummary/BlockNoteSummaryView';
import { invoke as invokeTauri } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { buildSummaryDocument, buildSummaryMarkdownBody } from '@/lib/summary-markdown';

interface UseSummaryExportProps {
  meeting: any;
  meetingTitle: string;
  aiSummary: Summary | null;
  blockNoteSummaryRef: RefObject<BlockNoteSummaryViewRef>;
}

/**
 * Exports the meeting summary as `summary.md` into the meeting's recording folder
 * (next to `transcripts.json`), via the `api_export_meeting_summary_markdown` command.
 *
 * - `exportSummaryFromMarkdown` writes a supplied body and is used for the automatic
 *   export right after a summary is generated (silent: no success toast, failures are
 *   swallowed so folderless meetings don't nag on every generation).
 * - `handleExportSummary` resolves the current summary markdown (including unsaved
 *   editor changes) and exports it, with user-facing toasts, for the manual button.
 */
export function useSummaryExport({
  meeting,
  meetingTitle,
  aiSummary,
  blockNoteSummaryRef,
}: UseSummaryExportProps) {
  const exportSummaryFromMarkdown = useCallback(
    async (body: string, options?: { silent?: boolean }): Promise<boolean> => {
      const silent = options?.silent ?? false;

      if (!body.trim()) {
        if (!silent) toast.error('No summary content available to export');
        return false;
      }

      const markdownDocument = buildSummaryDocument({
        title: meetingTitle,
        meetingId: meeting.id,
        createdAt: meeting.created_at,
        body,
        actionLabel: 'Exported on',
      });

      try {
        const path = await invokeTauri<string>('api_export_meeting_summary_markdown', {
          meetingId: meeting.id,
          markdown: markdownDocument,
        });
        console.log('✅ Summary exported to', path);

        if (!silent) {
          toast.success('Summary exported to meeting folder', {
            description: 'Saved as summary.md, next to the transcript.',
            action: {
              label: 'Open folder',
              onClick: () => {
                void invokeTauri('open_meeting_folder', { meetingId: meeting.id });
              },
            },
          });
        }
        return true;
      } catch (error) {
        // Folderless meetings can't export next to a transcript; stay quiet on auto-export.
        console.warn('Failed to export summary markdown:', error);
        if (!silent) {
          toast.error('Failed to export summary', { description: String(error) });
        }
        return false;
      }
    },
    [meeting.id, meeting.created_at, meetingTitle]
  );

  const handleExportSummary = useCallback(async () => {
    const body = await buildSummaryMarkdownBody({ aiSummary, blockNoteSummaryRef });
    await exportSummaryFromMarkdown(body);
  }, [aiSummary, blockNoteSummaryRef, exportSummaryFromMarkdown]);

  return {
    exportSummaryFromMarkdown,
    handleExportSummary,
  };
}
