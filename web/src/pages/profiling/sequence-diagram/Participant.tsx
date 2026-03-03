interface ParticipantProps {
  x: number;
  label: string;
  color: string;
  height: number;
}

export function Participant({ x, label, color, height }: ParticipantProps) {
  const boxW = 100;
  const boxH = 32;
  const boxX = x - boxW / 2;

  return (
    <g>
      {/* Box */}
      <rect
        x={boxX}
        y={4}
        width={boxW}
        height={boxH}
        rx={6}
        fill={color}
        fillOpacity={0.15}
        stroke={color}
        strokeWidth={1.5}
      />
      {/* Label */}
      <text
        x={x}
        y={24}
        textAnchor="middle"
        fill={color}
        fontSize={11}
        fontWeight={600}
        fontFamily="'Inter', sans-serif"
      >
        {label}
      </text>
      {/* Lifeline */}
      <line
        x1={x}
        y1={boxH + 8}
        x2={x}
        y2={height}
        stroke={color}
        strokeWidth={1}
        strokeDasharray="4 3"
        opacity={0.3}
      />
    </g>
  );
}
