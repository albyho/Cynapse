export interface OpenFileAction {
  type: 'OpenFile'
  path: string
}

export interface InsertTextAction {
  type: 'InsertText'
  path: string
  target: InsertTextTarget
  text: string
  speed_ms?: number
  target_hold_ms?: number
}

export type Placement = 'before' | 'after'

export interface TextAnchor {
  text: string
  index?: number
}

export type AnchorAt = number | TextAnchor

export interface Anchor {
  line?: number
  at?: AnchorAt
  placement?: Placement
  eof?: boolean
}

export type InsertTextTarget = Anchor

export interface SetCursorAction {
  type: 'SetCursor'
  path: string
  target: Anchor
}

export interface EditRange {
  start: Anchor
  end: Anchor
  expected_text: string
}

export interface DeleteTextAction {
  type: 'DeleteText'
  path: string
  target: DeleteTextTarget
  direction?: DeleteDirection
  speed_ms?: number
  target_hold_ms?: number
}

export type DeleteTextTarget =
  | EditRange
  | {
      cursor: {
        line: number
        column: number
        count: number
      }
      expected_text: string
    }

export type DeleteDirection = 'backspace' | 'delete'

export interface ReplaceTextAction {
  type: 'ReplaceText'
  path: string
  target: EditRange
  text: string
  edit_mode?: ReplaceTextEditMode
  speed_ms?: number
  target_hold_ms?: number
}

export type ReplaceTextEditMode = 'overwrite_selection' | 'delete_then_type'

export interface PreparedTarget {
  uri: string
  lineCount: number
  version: number
  savedText: string
  dirty: boolean
}

export interface Position {
  line: number
  character: number
}

export interface TextMatch {
  matched: number
  position: Position
}

export interface InsertOptions {
  undoStopBefore: boolean
  undoStopAfter: boolean
}

export type PreviewView = 'side' | 'current'

export interface OpenPreviewAction {
  type: 'OpenPreview'
  path: string
  view: PreviewView
}

export interface OpenFileWithPreviewAction {
  type: 'OpenFileWithPreview'
  path: string
}

export interface CloseFileAction {
  type: 'CloseFile'
  path: string
}

export interface ClosePreviewAction {
  type: 'ClosePreview'
  path: string
}

export interface RevealPosition {
  line: number
  column?: number
}

export interface RevealTextAction {
  type: 'RevealText'
  path: string
  start: RevealPosition
  end?: RevealPosition
}

export interface ClearRevealAction {
  type: 'ClearReveal'
}

export interface EditorFacade {
  open(filePath: string, placement?: 'first'): Promise<PreparedTarget>
  closeSidePreviews(): Promise<void>
  closeFile(filePath: string): Promise<void>
  closePreview(filePath: string): Promise<void>
  preview(filePath: string, view: PreviewView): Promise<void>
  reveal(target: PreparedTarget, start: Position, end: Position): Promise<void>
  clearReveal(): Promise<void>
  existingTarget(filePath: string): Promise<PreparedTarget>
  insert(
    target: PreparedTarget,
    position: Position,
    text: string,
    options: InsertOptions,
  ): Promise<void>
  matchAt(
    target: PreparedTarget,
    position: Position,
    expected: string,
  ): Promise<TextMatch>
  text(target: PreparedTarget): Promise<string>
  replace(
    target: PreparedTarget,
    start: Position,
    end: Position,
    text: string,
    options: InsertOptions,
  ): Promise<void>
  select(target: PreparedTarget, start: Position, end: Position): Promise<void>
  save(target: PreparedTarget): Promise<void>
  endUndoGroup(target: PreparedTarget): Promise<void>
  delay(ms: number, signal?: AbortSignal): Promise<void>
}

export type ApplyOutcome = 'done' | 'cancelled'

export class ApplyError extends Error {
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message)
    this.name = 'ApplyError'
  }
}
