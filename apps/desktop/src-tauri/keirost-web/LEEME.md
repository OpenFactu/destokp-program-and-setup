# Web de Keirost empaquetada

Este directorio lo rellena el pipeline de release con el contenido del artefacto
`keirost-web-<versión>.zip` antes de compilar la aplicación.

En desarrollo está vacío a propósito: sin `index.html`, la aplicación detecta
que no hay web empaquetada y carga directamente la del servidor al que se
conecta, de modo que se puede trabajar sin tener que descargar el artefacto.
