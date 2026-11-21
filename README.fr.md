# Fuu (封)

[![License: GPL-3.0](https://img.shields.io/github/license/palf-gh/Fuu)](LICENSE)
[![Release](https://img.shields.io/github/v/release/palf-gh/Fuu?include_prereleases&label=release)](https://github.com/palf-gh/Fuu/releases)
[![Downloads](https://img.shields.io/github/downloads/palf-gh/Fuu/total)](https://github.com/palf-gh/Fuu/releases)
[![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-dea584?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)](https://github.com/palf-gh/Fuu/releases)

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-Hans.md) | [繁體中文](README.zh-Hant.md) | [한국어](README.ko.md) | [Português (Brasil)](README.pt-BR.md) | [Español (España)](README.es-ES.md) | [Español (Latinoamérica)](README.es-419.md) | [Deutsch](README.de.md) | **Français** | [Bahasa Indonesia](README.id.md)

> Ce README est une traduction par IA de l’[original japonais](README.ja.md). Les corrections sont les bienvenues.

Fuu est un client de messagerie épuré pour Windows, macOS et Linux. Il se concentre sur la lecture, l’écriture et la réponse aux e-mails. Vous pouvez utiliser plusieurs comptes et donner à chacun sa propre signature. Fuu se connecte directement à votre serveur de messagerie en IMAP / SMTP, sans aucun autre serveur entre les deux. Son nom vient du sens du kanji 封 : sceller, fermer et protéger.

> **Ceci est une version alpha.** Les bases sont en place, mais attendez-vous à des bogues et à des changements de comportement.

## Téléchargement

Choisissez le fichier correspondant à votre système dans [Releases](https://github.com/palf-gh/Fuu/releases).

| Système | Fichier |
|---|---|
| Windows 10 / 11 (x64) | `fuu_<version>_x64-setup.exe` (aucun droit administrateur requis) |
| macOS 11 ou ultérieur (Apple silicon / Intel) | `Fuu_<version>_aarch64.dmg` / `Fuu_<version>_x86_64.dmg` |
| Linux (x86_64) | `fuu_<version>_amd64.deb` / `fuu_<version>_x86_64.AppImage` |

- La version macOS n’est pas encore signée ni notariée par Apple. À la première ouverture, choisissez « Ouvrir quand même » dans Réglages Système → Confidentialité et sécurité.
- L’installateur Windows n’est pas signé non plus, si bien que SmartScreen peut afficher un avertissement.
- Sous Linux, l’enregistrement des mots de passe nécessite un Secret Service, comme GNOME Keyring ou KWallet.

## Ce qu’il fait

- Lire, rédiger, répondre, répondre à tous et transférer
- Plusieurs comptes, avec connexion par mot de passe (mot de passe d’application) ou par OAuth 2.0 Google / Microsoft
- Une signature par compte
- Les e-mails HTML ne s’affichent qu’après suppression des parties dangereuses ; les images externes ne se chargent que si vous l’autorisez
- Aperçu des pièces jointes (images, PDF, Word, Excel, PowerPoint)
- Une fenêtre de rédaction avec gras, listes, etc.
- Les nouveaux messages de la boîte de réception apparaissent d’eux-mêmes (IMAP IDLE)
- La liste est groupée par date ; archivez et supprimez directement depuis la liste, et remettez des messages de la Corbeille dans la boîte de réception
- Ouvre les liens e-mail (`mailto:`) et peut être votre app de messagerie par défaut
- Recherche les nouvelles versions et se met à jour après vérification de la signature

## Langues

Fuu est disponible en anglais, japonais, chinois (simplifié et traditionnel), coréen, portugais (Brésil), espagnol (Espagne et Amérique latine), allemand, français et indonésien. Il suit la langue du système, et vous pouvez la changer dans Réglages → Langue.

Les traductions autres que le japonais, ainsi que le README de chaque langue, ont été réalisées par IA. Si vous remarquez quelque chose de peu naturel ou d’erroné, signalez-le-nous par une issue ou une pull request. Les fichiers de traduction se trouvent dans [`lang/`](lang).

## Raccourcis clavier

| Touche | Action |
|---|---|
| `j` / `k` (ou ↓ / ↑) | Message suivant / précédent |
| `r` | Répondre |
| `a` | Répondre à tous |
| `f` | Transférer |
| `e` | Archiver (dans la Corbeille et les Indésirables : remettre dans la boîte de réception) |
| `Delete` / `#` | Supprimer |
| `c` | Nouveau message |
| `F5` | Recharger le dossier affiché |
| `Ctrl+Enter` (`Cmd+Enter` sous macOS) | Envoyer |
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Ouvrir un onglet / le fermer / rouvrir un onglet fermé |
| `Ctrl+,` | Réglages |

Les touches d’une seule lettre ne fonctionnent que lorsqu’aucun champ de texte n’est sélectionné.

## Faire de Fuu votre app de messagerie par défaut

Cliquez sur « Définir Fuu par défaut » dans Réglages → Général, et les liens e-mail s’ouvriront dans Fuu.

- **Windows** : Fuu s’enregistre comme app de messagerie, puis la page Applications par défaut s’ouvre. Choisissez-y Fuu. Windows ne permet pas à une app de se définir elle-même par défaut.
- **macOS** : disponible lorsque Fuu est lancé depuis `Fuu.app`.
- **Linux** : Fuu place son fichier desktop dans `~/.local/share/applications` et le définit par défaut avec `xdg-mime`.

Un lien ne remplit que les destinataires, Cc, Cci, l’objet et le corps ; les demandes de joindre des fichiers sont ignorées. L’envoi reste toujours entre vos mains.

## Mises à jour

Peu après son démarrage, Fuu vérifie sur GitHub s’il existe une nouvelle version. Si c’est le cas, un bouton de mise à jour apparaît dans la barre de titre. En cliquant dessus, Fuu télécharge la mise à jour, vérifie sa signature et la met en place. Dans Réglages → Général, vous pouvez désactiver cette vérification ou vérifier tout de suite. Si vous avez installé le `.deb`, Fuu ouvre plutôt la page Releases.

La vérification envoie une seule requête HTTPS à GitHub. Aucune donnée d’utilisation n’est envoyée.

## Connexion avec OAuth (Gmail, Outlook.com)

Fuu n’est pas livré avec des identifiants client OAuth. Enregistrez les vôtres et placez-les dans `oauth.toml`, dans le même dossier que `accounts.toml` (Windows : `%APPDATA%\Fuu\oauth.toml` ; macOS : `~/Library/Application Support/Fuu/oauth.toml` ; Linux : `~/.config/Fuu/oauth.toml`).

```toml
[google]
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # les clients de bureau en reçoivent un aussi ; il n’est pas secret

[microsoft]
client_id = "00000000-0000-0000-0000-000000000000"
```

Les variables d’environnement `FUU_GOOGLE_CLIENT_ID`, `FUU_GOOGLE_CLIENT_SECRET` et `FUU_MICROSOFT_CLIENT_ID` ont priorité sur le fichier. Redémarrez Fuu après l’avoir modifié.

**Google** (console Google Cloud)

1. Créez un projet, activez la Gmail API et configurez l’écran de consentement OAuth (type « External » ; ajoutez-vous comme utilisateur test).
2. Ajoutez le champ d’application `https://mail.google.com/`.
3. Dans « Credentials » → « OAuth client ID », créez une « Desktop app ». Aucun URI de redirection n’est à enregistrer (Fuu utilise `http://127.0.0.1:<port quelconque>/`).
4. Tant que l’app est en « Testing », il vous sera demandé de vous reconnecter tous les 7 jours. C’est une règle de Google, pas un bogue de Fuu.

**Microsoft** (centre d’administration Microsoft Entra → « App registrations »)

1. Inscrivez une nouvelle app avec les types de comptes pris en charge « Comptes dans un annuaire d’organisation et comptes Microsoft personnels ».
2. Dans « Authentication » → « Add a platform » → « Mobile and desktop applications », ajoutez `http://localhost` comme URI de redirection. Activez « Allow public client flows ». Aucun secret client n’est nécessaire.
3. Dans « API permissions », ajoutez les autorisations déléguées d’*Office 365 Exchange Online* `IMAP.AccessAsUser.All` et `SMTP.Send` (ainsi que `offline_access`).

La connexion se fait dans votre navigateur habituel. Le jeton d’actualisation n’est conservé que dans le trousseau du système.

## Pour les développeurs

La compilation, les tests, l’empaquetage, la publication et la traduction sont décrits dans [docs/development.md](docs/development.md) (en anglais). Voir [ARCHITECTURE.md](ARCHITECTURE.md) pour la conception et [SECURITY.md](SECURITY.md) pour la façon dont Fuu vous protège.

## Licence

GPL-3.0-only. Fuu utilise Slint sous la GPLv3.
