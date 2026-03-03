interface TimeMarkerProps {
  y: number;
  label: string;
}

export function TimeMarker({ y, label }: TimeMarkerProps) {
  return (
    <text
      x={4}
      y={y + 3}
      fill="#606068"
      fontSize={9}
      fontFamily="'SF Mono', 'Fira Code', monospace"
    >
      {label}
    </text>
  );
}
