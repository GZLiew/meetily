import { RefObject } from 'react';
import { Summary } from '@/types';
import { BlockNoteSummaryViewRef } from '@/components/AISummary/BlockNoteSummaryView';

// Keys on a Summary object that are not renderable content sections.
const NON_SECTION_KEYS = new Set(['markdown', 'summary_json', '_section_order', 'MeetingName']);

/**
 * Converts a legacy structured summary object (`{ section: { title, blocks } }`)
 * into markdown, one `## Title` heading per section followed by its bullet blocks.
 */
export function legacySummaryToMarkdown(summary: Summary): string {
  return Object.entries(summary)
    .filter(([key]) => !NON_SECTION_KEYS.has(key))
    .map(([, section]) => {
      if (section && typeof section === 'object' && 'title' in section && 'blocks' in section) {
        const sectionTitle = `## ${(section as any).title}\n\n`;
        const sectionContent = ((section as any).blocks as any[])
          .map((block: any) => `- ${block.content}`)
          .join('\n');
        return sectionTitle + sectionContent;
      }
      return '';
    })
    .filter((s) => s.trim())
    .join('\n\n');
}

/**
 * Resolves the summary body as markdown from the best available source, in order:
 * 1. The live BlockNote editor (captures unsaved edits), 2. an explicit `markdown`
 * field on the summary, 3. legacy structured-section conversion. Returns '' if empty.
 */
export async function buildSummaryMarkdownBody({
  aiSummary,
  blockNoteSummaryRef,
}: {
  aiSummary: Summary | null;
  blockNoteSummaryRef?: RefObject<BlockNoteSummaryViewRef>;
}): Promise<string> {
  let body = '';

  if (blockNoteSummaryRef?.current?.getMarkdown) {
    body = await blockNoteSummaryRef.current.getMarkdown();
  }

  if (!body && aiSummary && 'markdown' in aiSummary) {
    body = (aiSummary as any).markdown || '';
  }

  if (!body && aiSummary) {
    body = legacySummaryToMarkdown(aiSummary);
  }

  return body;
}

const formatSummaryDate = (date: Date): string =>
  date.toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });

/**
 * Wraps a summary body in a titled document with metadata (meeting id, date, and the
 * time of the action) for copy-to-clipboard or file export. `actionLabel` is the verb
 * shown for the current timestamp, e.g. 'Copied on' or 'Exported on'.
 */
export function buildSummaryDocument({
  title,
  meetingId,
  createdAt,
  body,
  actionLabel,
}: {
  title: string;
  meetingId: string;
  createdAt: string;
  body: string;
  actionLabel: string;
}): string {
  const header = `# Meeting Summary: ${title}\n\n`;
  const metadata =
    `**Meeting ID:** ${meetingId}\n` +
    `**Date:** ${formatSummaryDate(new Date(createdAt))}\n` +
    `**${actionLabel}:** ${formatSummaryDate(new Date())}\n\n---\n\n`;
  return header + metadata + body;
}
