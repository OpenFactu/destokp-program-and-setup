/**
 * La marca de Keirost: K de trazo redondeado con punto teal en el cruce. Es
 * `KeirostLogo` de `platform`; si cambia allí, hay que cambiarla aquí. La K va
 * con el color de texto del tema y no con `currentColor`: `PageHeader` tiñe de
 * acento lo que recibe, y una K teal se comería el punto.
 */
export function LogoKeirost({ className }: { className?: string }) {
  const trazo = {
    fill: 'none',
    stroke: 'var(--fg-default, #0a1628)',
    strokeWidth: 13,
    strokeLinecap: 'round',
    strokeLinejoin: 'round',
  } as const;

  return (
    <svg
      viewBox="12 11 78 78"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      role="img"
      aria-label="Keirost"
    >
      <path d="M30 20V80" {...trazo} />
      <path d="M72 20L50 50L72 80" {...trazo} />
      <circle cx="50" cy="50" r="5" fill="#0D9488" />
    </svg>
  );
}
