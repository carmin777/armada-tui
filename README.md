# armada-tui

Cliente de terminal em Rust/Ratatui para comunidades Concord E2EE e grupos NIP-29 em relays Nostr.

> Estado atual: protótipo funcional. Há caminhos live testados, mas o projeto ainda não é um cliente completo nem recebeu auditoria criptográfica independente.

## O que este projeto faz

O binário oferece uma TUI de nove telas, com dados mock para exploração local e operações live disparadas sob demanda:

- leitura, autenticação NIP-42, escrita e pedido de entrada em grupos NIP-29;
- leitura e escrita de streams Concord com NIP-44 v2, seals e gift-wraps;
- abertura de invites Concord, descoberta do control plane e canais públicos;
- DMs E2EE NIP-17/NIP-44;
- presença de chamadas NIP-29 (evento 39004);
- download de uma imagem HTTP(S) e visualização via Kitty Graphics;
- exemplos headless para exercitar relay, DMs, invites e a sinalização de voz.

A matriz detalhada, com o status e a evidência de cada recurso, está em [`parity.json`](parity.json). `done` significa que existe evidência; `partial` e `missing` são estados intencionais do protótipo.

## O que ainda não faz

- Não há áudio ou vídeo WebRTC dentro do terminal. O módulo [`src/voice.rs`](src/voice.rs) implementa sinalização de suporte e obtenção de token LiveKit; ele não abre uma sessão LiveKit.
- Discover, Projects e Inbox continuam parcialmente mockados.
- Ainda não há login NIP-07, NIP-46/bunker, upload Blossom, busca NIP-50, zaps, notificações, rekey completo ou todos os grants privados Concord.
- O login não persiste a chave em disco. Sem uma chave válida, a aplicação permanece em modo mock/leitura.

## Requisitos

- Rust stable e Cargo;
- um terminal compatível com crossterm;
- opcionalmente, Kitty ou outro terminal com Kitty Graphics para a visualização de imagens;
- Node.js/npm e Chrome/Chromium apenas para a checagem sintática e os scripts E2E manuais em `e2e/`.

## Executar

```bash
cargo run -- --relay wss://relay.ditto.pub
```

Ao iniciar, a TUI usa dados locais para ser explorável sem rede. Na tela do servidor:

1. pressione `r` para buscar grupos NIP-29 no relay configurado;
2. selecione um grupo e pressione `m` para buscar mensagens;
3. use `J` para enviar um pedido NIP-29 `9021`;
4. faça login com uma chave somente quando quiser publicar ou acessar dados protegidos.

O parâmetro `--relay` troca apenas o primeiro relay de aplicação. Relays precisam passar pela política de rede em [`src/netpolicy.rs`](src/netpolicy.rs): esquemas e hosts locais/privados são recusados.

## Login e higiene de segredos

O campo de login aceita `nsec1...` ou uma chave hexadecimal de 32 bytes. A tecla `g` com o campo vazio gera uma identidade descartável para demonstração.

Regras para uso seguro:

- nunca coloque uma nsec em argumentos da linha de comando, issues, screenshots, logs ou commits;
- nunca reutilize uma nsec de produção nos exemplos E2E; prefira a conta descartável gerada pelo script;
- um invite Concord (`naddr#fragment`) contém uma capability. Trate o link inteiro como segredo e não o publique;
- `npub` e chaves públicas não são segredos, mas podem identificar atividade e também não devem ser colados sem contexto;
- tokens JWT LiveKit e headers NIP-98 são credenciais temporárias. O código evita imprimir o JWT inteiro, mas o operador ainda deve proteger a saída do processo;
- os vetores criptográficos em `src/concord/fixture.rs` são determinísticos e destinados apenas a testes. Não são identidades ou chaves de produção.

A chave de sessão é mantida apenas em memória e envolvida em `Zeroizing`; o logout cancela workers e limpa o estado da sessão. Isso reduz exposição acidental, mas não transforma o programa em um cofre de chaves.

## Atalhos da TUI

| Tecla | Ação |
| --- | --- |
| `1`–`8` | trocar de tela |
| `?` | ajuda |
| `Tab` | alternar foco entre frota, canais e mensagens |
| `j`/`k` ou setas | navegar |
| `i` / `Enter` | digitar; `Enter` envia |
| `r` | buscar grupos, DMs ou presença conforme a tela |
| `m` | buscar mensagens |
| `I` | abrir um invite Concord |
| `J` | pedir entrada no grupo NIP-29 selecionado |
| `v` | baixar e visualizar a primeira imagem encontrada |
| `o` | logout na tela Settings |
| `q` / `Ctrl-C` / `Ctrl-Q` | sair e encerrar a sessão |

## Exemplos headless

Defina apenas variáveis públicas ou de teste no shell:

```bash
RELAY=wss://relay.ditto.pub

cargo run --example nip29_live -- "$RELAY" groups
cargo run --example nip29_live -- "$RELAY" msgs '<group-id>'
cargo run --example nip29_live -- "$RELAY" voice '<group-id>'
cargo run --example nip29_live -- "$RELAY" join '<group-id>'
cargo run --example nip29_live -- "$RELAY" send '<group-id>' 'mensagem de teste'

# self-DM: gera uma identidade descartável e faz read-back
cargo run --example dm_live -- "$RELAY" 'mensagem de teste'

# abre um invite fornecido por você; não publique o link em logs
cargo run --example e2e_invite -- '<invite-url>'
```

`join` e `send` geram uma identidade descartável e podem publicar no relay. Execute apenas contra um grupo/relay de teste e respeite as regras do serviço. O exemplo de invite também aceita o nome do canal e `--send`, mas publicar é opcional e deve ser feito somente com autorização.

## Scripts E2E do cliente web

Os scripts em `e2e/` são manuais, não fazem parte do fluxo live do CI e acessam `https://armada.buzz`. Eles servem para verificar o comportamento do cliente web e produzir evidência de interoperabilidade.

```bash
npm ci --prefix e2e --no-audit --no-fund
npm test --prefix e2e

# Somente em uma conta descartável ou explicitamente autorizada:
NSEC='<chave-fora-do-repositorio>' node e2e/finish.cjs
```

O teste padrão do CI faz apenas `node --check` nos scripts. Screenshots, logs e `node_modules` são artefatos locais ignorados e não devem ser adicionados ao Git.

## Arquitetura

| Área | Arquivo(s) | Responsabilidade |
| --- | --- | --- |
| Entrada | [`src/main.rs`](src/main.rs) | CLI, terminal e loop de eventos |
| Estado/UI | [`src/app.rs`](src/app.rs), [`src/ui.rs`](src/ui.rs) | telas, atalhos, workers e renderização |
| Nostr | [`src/nostr.rs`](src/nostr.rs) | WebSocket, validação NIP-01, NIP-29, NIP-42 e publicação |
| Concord | [`src/concord/`](src/concord/) | derivação, NIP-44, invites, control e streams |
| DMs | [`src/dm.rs`](src/dm.rs) | NIP-17/NIP-59/NIP-44 e agrupamento de conversas |
| Voz | [`src/voice.rs`](src/voice.rs) | probe LiveKit, NIP-98, parsing de token e claims |
| Rede | [`src/netpolicy.rs`](src/netpolicy.rs) | allowlist de esquemas e bloqueio de hosts locais |
| Dados locais | [`src/mock.rs`](src/mock.rs) | conteúdo demonstrativo sem credenciais |

As operações de rede rodam em workers com timeouts e cancelamento por logout, para que a TUI continue responsiva. Eventos recebidos são validados antes de serem aceitos; filtros e deduplicação também são conferidos localmente.

## Protocolos implementados

- NIP-29: anúncios `39000`, mensagens com `#h`, presença `39004`, join `9021` e autenticação relay `22242`;
- NIP-98: autorização HTTP com evento efêmero `27235` para o endpoint LiveKit;
- Concord: bundle `33301`, gift-wraps `1059/21059`, seals `20013/20014` e binding de canal/época;
- NIP-44 v2: ECDH, HKDF, padding, ChaCha20 e HMAC-SHA256;
- DMs: seal/wrap NIP-17/NIP-59 com leitura autenticada quando o relay exige NIP-42.

Esta lista descreve o que o código tenta implementar; compatibilidade de protocolo não equivale a uma auditoria de segurança.

## Desenvolvimento e verificação

Antes de abrir um pull request:

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
npm test --prefix e2e
```

O workflow em [`.github/workflows/ci.yml`](.github/workflows/ci.yml) executa essas verificações, mas não abre navegador nem publica em relays.

## Dados externos e privacidade

Os defaults apontam para relays/blossom públicos usados pelo cliente de referência. Ao usar a aplicação live, metadados, mensagens, IP e horários podem ser observados pelos serviços e relays escolhidos. O projeto não oferece anonimato de rede.

Não há telemetria própria implementada. Mesmo assim, o relay e o servidor HTTP acessados recebem a conexão normal da rede. Use `--relay` e uma configuração própria se isso for necessário.

## Licença

Distribuído sob a GNU Affero General Public License v3 ou posterior. Consulte [`LICENSE`](LICENSE).
