import uiPreset from '@openfactu/ui/tailwind-preset';
import animated from 'tailwindcss-animated';

/** @type {import('tailwindcss').Config} */
export default {
  // El preset trae los tokens del sistema de diseño de Keirost: colores,
  // tipografía y espaciados son los mismos que los del ERP.
  presets: [uiPreset],
  content: [
    './index.html',
    './src/**/*.{ts,tsx}',
    // Sin esto, el JIT no ve las clases que usan los componentes del paquete y
    // la interfaz sale sin estilos.
    './node_modules/@openfactu/ui/dist/**/*.{js,jsx}',
    '../../../node_modules/@openfactu/ui/dist/**/*.{js,jsx}',
  ],
  plugins: [animated],
};
