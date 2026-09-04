# Política de segurança

## Escopo

Este projeto manipula chaves privadas Nostr, conteúdo E2EE, invites/capabilities e tokens temporários LiveKit. Erros nesses caminhos podem causar perda de identidade, publicação não autorizada ou exposição de metadados.

## Não publique segredos

Não abra uma issue pública contendo:

- `nsec`, chave privada hexadecimal ou seed;
- invite Concord completo (`naddr#fragment`);
- JWT, header NIP-98, cookie, token de relay ou arquivo de configuração;
- screenshot/log que contenha qualquer um desses valores.

Se um segredo foi exposto, revogue ou substitua-o imediatamente. Considere todo invite publicado comprometido.

## Como reportar

Para uma vulnerabilidade, use um canal privado do GitHub (Security Advisories, quando habilitado) ou o contato privado indicado pelos mantenedores do repositório. Não divulgue detalhes exploráveis antes de existir uma correção ou um acordo de divulgação.

Inclua, sem incluir segredos:

- commit ou versão afetada;
- sistema operacional e versão do Rust;
- passos mínimos para reproduzir;
- impacto observado;
- logs sanitizados e um caso de teste sintético, se possível.

## Limitações conhecidas

- O projeto ainda não passou por auditoria criptográfica independente.
- O terminal não implementa áudio/vídeo LiveKit; a voz atual é sinalização/presença.
- A assinatura do JWT LiveKit é transportada pelo endpoint HTTPS e a identidade é conferida, mas o módulo de voz não verifica localmente a assinatura do JWT.
- Relays e servidores HTTP externos continuam podendo observar metadados de conexão e disponibilidade.

## Regra para contribuições

Testes devem usar dados determinísticos ou identidades descartáveis. Nunca comite uma chave para “facilitar” um teste. Consulte também [`README.md`](README.md) e [`CONTRIBUTING.md`](CONTRIBUTING.md).
