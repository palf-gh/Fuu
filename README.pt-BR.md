# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-Hans.md) | [繁體中文](README.zh-Hant.md) | [한국어](README.ko.md) | **Português (Brasil)** | [Español (España)](README.es-ES.md) | [Español (Latinoamérica)](README.es-419.md) | [Deutsch](README.de.md) | [Français](README.fr.md) | [Bahasa Indonesia](README.id.md)

> Este README é uma tradução feita por IA do [original em japonês](README.ja.md). Correções são bem-vindas.

O Fuu é um cliente de e-mail enxuto para Windows, macOS e Linux. Ele se concentra em ler, escrever e responder e-mails. Você pode usar várias contas e definir uma assinatura para cada uma. O Fuu se conecta diretamente ao seu servidor de e-mail por IMAP / SMTP, sem nenhum outro servidor no meio. O nome vem do significado do kanji 封: selar, fechar e proteger.

> **Esta é uma versão alfa.** O básico já funciona, mas pode haver bugs e mudanças de comportamento.

## Download

Escolha o arquivo do seu sistema em [Releases](https://github.com/palf-gh/Fuu/releases).

| Sistema | Arquivo |
|---|---|
| Windows 10 / 11 (x64) | `fuu_<versão>_x64-setup.exe` (não precisa de permissão de administrador) |
| macOS 11 ou posterior (Apple silicon / Intel) | `Fuu_<versão>_aarch64.dmg` / `Fuu_<versão>_x86_64.dmg` |
| Linux (x86_64) | `fuu_<versão>_amd64.deb` / `fuu_<versão>_x86_64.AppImage` |

- A versão para macOS ainda não foi assinada nem autenticada (notarizada) pela Apple. Na primeira vez que abrir, escolha “Abrir Mesmo Assim” em Ajustes do Sistema → Privacidade e Segurança.
- O instalador do Windows também não é assinado, então o SmartScreen pode exibir um aviso.
- No Linux, salvar senhas exige um Secret Service, como o GNOME Keyring ou o KWallet.

## O que ele faz

- Ler, escrever, responder, responder a todos e encaminhar
- Várias contas, com login por senha (senha de app) ou por OAuth 2.0 do Google / Microsoft
- Uma assinatura por conta
- E-mails em HTML só são exibidos depois que as partes perigosas são removidas; imagens externas só carregam quando você permite
- Pré-visualização de anexos (imagens, PDF, Word, Excel, PowerPoint)
- Uma janela de escrita com negrito, listas e afins
- Novos e-mails na caixa de entrada aparecem sozinhos (IMAP IDLE)
- A lista é agrupada por data; arquive e exclua direto da lista, e devolva e-mails da Lixeira para a caixa de entrada
- Abre links de e-mail (`mailto:`) e pode ser o seu app de e-mail padrão
- Verifica se há novas versões e se atualiza depois de conferir a assinatura

## Idiomas

O Fuu está disponível em inglês, japonês, chinês (simplificado e tradicional), coreano, português (Brasil), espanhol (Espanha e América Latina), alemão, francês e indonésio. Ele segue o idioma do sistema, e você pode trocar em Configurações → Idioma.

As traduções que não são em japonês e o README de cada idioma foram feitos com IA. Se notar algo estranho ou errado, avise-nos por uma issue ou um pull request. Os arquivos de tradução ficam em [`lang/`](lang).

## Atalhos de teclado

| Tecla | Ação |
|---|---|
| `j` / `k` (ou ↓ / ↑) | Próxima / anterior mensagem |
| `r` | Responder |
| `a` | Responder a todos |
| `f` | Encaminhar |
| `e` | Arquivar (na Lixeira e no Spam: devolver à caixa de entrada) |
| `Delete` / `#` | Excluir |
| `c` | Nova mensagem |
| `F5` | Recarregar a pasta exibida |
| `Ctrl+Enter` (`Cmd+Enter` no macOS) | Enviar |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Abrir uma aba / fechá-la / reabrir uma aba fechada |
| `Ctrl+,` | Configurações |

As teclas de uma letra só funcionam quando nenhum campo de texto está selecionado.

## Tornar o Fuu seu app de e-mail padrão

Clique em “Tornar o Fuu padrão” em Configurações → Geral, e os links de e-mail passarão a abrir no Fuu.

- **Windows**: o Fuu se registra como app de e-mail e, em seguida, abre a página Aplicativos padrão. Escolha o Fuu ali. O Windows não permite que um app se torne padrão sozinho.
- **macOS**: disponível quando o Fuu é iniciado pelo `Fuu.app`.
- **Linux**: o Fuu coloca seu arquivo desktop em `~/.local/share/applications` e o define como padrão com `xdg-mime`.

Um link só preenche os destinatários, CC, CCO, assunto e corpo; pedidos para anexar arquivos são ignorados. O envio é sempre feito por você.

## Atualizações

Pouco depois de iniciar, o Fuu verifica no GitHub se há uma nova versão. Se houver, um botão “Atualizar” aparece na barra de título. Ao clicar, ele baixa a atualização, confere a assinatura e faz a substituição. Em Configurações → Geral, você pode desativar a verificação ou verificar na hora. Se você instalou pelo `.deb`, o Fuu abre a página de Releases em vez disso.

A verificação envia uma única requisição HTTPS ao GitHub. Nenhum dado de uso é enviado.

## Login com OAuth (Gmail, Outlook.com)

O Fuu não vem com IDs de cliente OAuth. Registre os seus e coloque-os em `oauth.toml`, na mesma pasta que `accounts.toml` (Windows: `%APPDATA%\Fuu\oauth.toml`; macOS: `~/Library/Application Support/Fuu/oauth.toml`; Linux: `~/.config/Fuu/oauth.toml`).

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # clientes de desktop também recebem um; não é secreto

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

As variáveis de ambiente `FUU_GOOGLE_CLIENT_ID`, `FUU_GOOGLE_CLIENT_SECRET` e `FUU_MICROSOFT_CLIENT_ID` têm prioridade sobre o arquivo. Reinicie o Fuu depois de editá-lo.

**Google** (console do Google Cloud)

1. Crie um projeto, ative a Gmail API e configure a tela de consentimento OAuth (tipo “External”; adicione você mesmo como usuário de teste).
2. Adicione o escopo `https://mail.google.com/`.
3. Em “Credentials” → “OAuth client ID”, crie um “Desktop app”. Não é preciso registrar um URI de redirecionamento (o Fuu usa `http://127.0.0.1:<qualquer porta>/`).
4. Enquanto o app estiver em “Testing”, será pedido que você faça login de novo a cada 7 dias. Essa é uma regra do Google, não um bug do Fuu.

**Microsoft** (centro de administração do Microsoft Entra → “App registrations”)

1. Registre um novo app com os tipos de conta compatíveis “Contas em qualquer diretório organizacional e contas pessoais da Microsoft”.
2. Em “Authentication” → “Add a platform” → “Mobile and desktop applications”, adicione `http://localhost` como URI de redirecionamento. Ative “Allow public client flows”. Nenhum segredo do cliente é necessário.
3. Em “API permissions”, adicione as permissões delegadas do *Office 365 Exchange Online* `IMAP.AccessAsUser.All` e `SMTP.Send` (e `offline_access`).

O login é feito no navegador que você usa normalmente. O token de atualização fica guardado apenas no chaveiro do sistema.

## Para desenvolvedores

Compilação, testes, empacotamento, lançamento e tradução estão descritos em [docs/development.md](docs/development.md) (em inglês). Veja [ARCHITECTURE.md](ARCHITECTURE.md) para o design e [SECURITY.md](SECURITY.md) para saber como o Fuu protege você.

## Licença

GPL-3.0-only. O Fuu usa o Slint sob a GPLv3.
