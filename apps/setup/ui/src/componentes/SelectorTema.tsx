import { SegmentedControl } from '@openfactu/ui';
import { Monitor, Moon, Sun } from 'lucide-react';

import { useTema, type Preferencia } from '../tema';

/** Claro / oscuro / el de Windows. */
export function SelectorTema() {
  const [preferencia, cambiar] = useTema();

  return (
    <SegmentedControl<Preferencia>
      size="sm"
      variant="raised"
      aria-label="Tema de la interfaz"
      value={preferencia}
      onChange={cambiar}
      options={[
        { value: 'claro', label: '', icon: <Sun className="h-4 w-4" />, title: 'Claro' },
        { value: 'oscuro', label: '', icon: <Moon className="h-4 w-4" />, title: 'Oscuro' },
        {
          value: 'sistema',
          label: '',
          icon: <Monitor className="h-4 w-4" />,
          title: 'El de Windows',
        },
      ]}
    />
  );
}
