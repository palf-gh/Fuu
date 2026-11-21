# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-Hans.md) | [繁體中文](README.zh-Hant.md) | [한국어](README.ko.md) | [Português (Brasil)](README.pt-BR.md) | [Español (España)](README.es-ES.md) | **Español (Latinoamérica)** | [Deutsch](README.de.md) | [Français](README.fr.md) | [Bahasa Indonesia](README.id.md)

> Este README es una traducción hecha con IA del [original en japonés](README.ja.md). Las correcciones son bienvenidas.

Fuu es un cliente de correo sencillo para Windows, macOS y Linux. Se enfoca en leer, escribir y responder correos. Puedes usar varias cuentas y asignar a cada una su propia firma. Fuu se conecta directamente a tu servidor de correo por IMAP / SMTP, sin ningún otro servidor de por medio. El nombre viene del significado del kanji 封: sellar, cerrar y proteger.

> **Esta es una versión alfa.** Lo básico ya funciona, pero puede haber errores y cambios de comportamiento.

## Descarga

Elige el archivo para tu sistema en [Releases](https://github.com/palf-gh/Fuu/releases).

| Sistema | Archivo |
|---|---|
| Windows 10 / 11 (x64) | `fuu_<versión>_x64-setup.exe` (no se necesitan permisos de administrador) |
| macOS 11 o posterior (Apple silicon / Intel) | `Fuu_<versión>_aarch64.dmg` / `Fuu_<versión>_x86_64.dmg` |
| Linux (x86_64) | `fuu_<versión>_amd64.deb` / `fuu_<versión>_x86_64.AppImage` |

- La versión de macOS todavía no está firmada ni notarizada por Apple. La primera vez que la abras, elige «Abrir de todos modos» en Configuración del Sistema → Privacidad y seguridad.
- El instalador de Windows tampoco está firmado, así que SmartScreen puede mostrar una advertencia.
- En Linux, para guardar contraseñas se necesita un Secret Service, como GNOME Keyring o KWallet.

## Qué hace

- Leer, redactar, responder, responder a todos y reenviar
- Varias cuentas, con inicio de sesión mediante contraseña (contraseña de aplicación) u OAuth 2.0 de Google / Microsoft
- Una firma por cuenta
- Los correos HTML solo se muestran después de quitar las partes peligrosas; las imágenes externas solo se cargan si lo permites
- Vista previa de adjuntos (imágenes, PDF, Word, Excel, PowerPoint)
- Una ventana de redacción con negrita, listas y similares
- Los correos nuevos de la bandeja de entrada aparecen solos (IMAP IDLE)
- La lista se agrupa por fecha; archiva y elimina directamente desde la lista, y regresa correos de la Papelera a la bandeja de entrada
- Abre enlaces de correo (`mailto:`) y puede ser tu aplicación de correo predeterminada
- Busca versiones nuevas y se actualiza después de verificar la firma

## Idiomas

Fuu está disponible en inglés, japonés, chino (simplificado y tradicional), coreano, portugués (Brasil), español (España y Latinoamérica), alemán, francés e indonesio. Sigue el idioma del sistema, y puedes cambiarlo en Configuración → Idioma.

Las traducciones distintas del japonés y el README de cada idioma se hicieron con IA. Si notas algo poco natural o incorrecto, avísanos con un issue o un pull request. Los archivos de traducción están en [`lang/`](lang).

## Atajos de teclado

| Tecla | Acción |
|---|---|
| `j` / `k` (o ↓ / ↑) | Mensaje siguiente / anterior |
| `r` | Responder |
| `a` | Responder a todos |
| `f` | Reenviar |
| `e` | Archivar (en la Papelera y en Spam: regresar a la bandeja de entrada) |
| `Delete` / `#` | Eliminar |
| `c` | Mensaje nuevo |
| `F5` | Volver a cargar la carpeta mostrada |
| `Ctrl+Enter` (`Cmd+Enter` en macOS) | Enviar |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Abrir una pestaña / cerrarla / volver a abrir una pestaña cerrada |
| `Ctrl+,` | Configuración |

Las teclas de una sola letra solo funcionan cuando no hay ningún campo de texto seleccionado.

## Hacer que Fuu sea tu aplicación de correo predeterminada

Presiona «Hacer que Fuu sea la predeterminada» en Configuración → General, y los enlaces de correo se abrirán en Fuu.

- **Windows**: Fuu se registra como aplicación de correo y luego se abre la página Aplicaciones predeterminadas. Elige Fuu ahí. Windows no permite que una aplicación se vuelva predeterminada por sí sola.
- **macOS**: disponible cuando Fuu se inicia desde `Fuu.app`.
- **Linux**: Fuu coloca su archivo desktop en `~/.local/share/applications` y lo establece como predeterminado con `xdg-mime`.

Un enlace solo llena los destinatarios, CC, CCO, asunto y cuerpo; las solicitudes de adjuntar archivos se ignoran. El envío siempre lo haces tú.

## Actualizaciones

Poco después de iniciar, Fuu revisa en GitHub si hay una versión nueva. Si la hay, aparece un botón «Actualizar» en la barra de título. Al presionarlo, descarga la actualización, verifica su firma y la reemplaza. En Configuración → General puedes desactivar la revisión o revisar de inmediato. Si instalaste el `.deb`, Fuu abre la página de Releases en su lugar.

La revisión envía una sola solicitud HTTPS a GitHub. No se envía ningún dato de uso.

## Inicio de sesión con OAuth (Gmail, Outlook.com)

Fuu no incluye ID de cliente OAuth. Registra los tuyos y colócalos en `oauth.toml`, en la misma carpeta que `accounts.toml` (Windows: `%APPDATA%\Fuu\oauth.toml`; macOS: `~/Library/Application Support/Fuu/oauth.toml`; Linux: `~/.config/Fuu/oauth.toml`).

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # los clientes de escritorio también reciben uno; no es secreto

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

Las variables de entorno `FUU_GOOGLE_CLIENT_ID`, `FUU_GOOGLE_CLIENT_SECRET` y `FUU_MICROSOFT_CLIENT_ID` tienen prioridad sobre el archivo. Reinicia Fuu después de editarlo.

**Google** (consola de Google Cloud)

1. Crea un proyecto, habilita la Gmail API y configura la pantalla de consentimiento OAuth (tipo «External»; agrégate como usuario de prueba).
2. Agrega el alcance `https://mail.google.com/`.
3. En «Credentials» → «OAuth client ID», crea una «Desktop app». No es necesario registrar ningún URI de redirección (Fuu usa `http://127.0.0.1:<cualquier puerto>/`).
4. Mientras la aplicación esté en «Testing», se te pedirá que vuelvas a iniciar sesión cada 7 días. Es una regla de Google, no un error de Fuu.

**Microsoft** (centro de administración de Microsoft Entra → «App registrations»)

1. Registra una aplicación nueva con los tipos de cuenta compatibles «Cuentas en cualquier directorio organizativo y cuentas personales de Microsoft».
2. En «Authentication» → «Add a platform» → «Mobile and desktop applications», agrega `http://localhost` como URI de redirección. Activa «Allow public client flows». No se necesita ningún secreto de cliente.
3. En «API permissions», agrega los permisos delegados de *Office 365 Exchange Online* `IMAP.AccessAsUser.All` y `SMTP.Send` (y `offline_access`).

El inicio de sesión se hace en tu navegador habitual. El token de actualización se guarda solo en el llavero del sistema.

## Para desarrolladores

La compilación, las pruebas, el empaquetado, la publicación y la traducción se describen en [docs/development.md](docs/development.md) (en inglés). Consulta [ARCHITECTURE.md](ARCHITECTURE.md) para el diseño y [SECURITY.md](SECURITY.md) para saber cómo te protege Fuu.

## Licencia

GPL-3.0-only. Fuu usa Slint bajo la GPLv3.
