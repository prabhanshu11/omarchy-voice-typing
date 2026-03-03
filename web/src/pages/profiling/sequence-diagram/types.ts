export interface Participant {
  id: string;
  label: string;
  color: string;
}

export interface DiagramArrow {
  fromId: string;
  toId: string;
  label: string;
  timestamp: number; // elapsed seconds
  durationMs?: number;
  style?: 'solid' | 'dashed';
}
