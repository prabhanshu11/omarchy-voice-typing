interface ArrowProps {
  x1: number;
  x2: number;
  y: number;
  label: string;
  color: string;
  dashed?: boolean;
}

export function Arrow({ x1, x2, y, label, color, dashed }: ArrowProps) {
  const isReverse = x2 < x1;
  const headSize = 6;

  // Arrow head direction
  const tipX = x2;
  const baseX = isReverse ? x2 + headSize : x2 - headSize;

  // Label position — centered on the arrow
  const midX = (x1 + x2) / 2;

  return (
    <g>
      {/* Line */}
      <line
        x1={x1}
        y1={y}
        x2={x2}
        y2={y}
        stroke={color}
        strokeWidth={1.5}
        strokeDasharray={dashed ? '4 2' : undefined}
      />
      {/* Arrowhead */}
      <polygon
        points={`${tipX},${y} ${baseX},${y - headSize / 2} ${baseX},${y + headSize / 2}`}
        fill={color}
      />
      {/* Label */}
      <text
        x={midX}
        y={y - 6}
        textAnchor="middle"
        fill="#9898a0"
        fontSize={10}
        fontFamily="'Inter', sans-serif"
      >
        {label}
      </text>
    </g>
  );
}
