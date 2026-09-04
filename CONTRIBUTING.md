# Contribuindo

Obrigado por ajudar. O objetivo é manter o cliente pequeno, legível e interoperável com os formatos existentes.

## Fluxo

1. Abra uma issue ou descreva o problema no pull request.
2. Faça uma alteração focada, preservando mudanças locais não relacionadas.
3. Adicione teste unitário para parsing, criptografia, validação ou máquina de estados sempre que possível.
4. Atualize `parity.json` somente quando houver evidência correspondente.
5. Explique limitações e efeitos de rede na documentação.

## Segurança

Não use contas pessoais, invites reais ou conteúdo privado nos testes. Chaves, tokens e URLs de invite são dados sensíveis, mesmo quando parecem ser “só para teste”. Veja [`SECURITY.md`](SECURITY.md).

## Checks locais

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
npm test --prefix e2e
```

Os scripts E2E completos acessam serviços externos e podem publicar eventos. Rode-os apenas manualmente, com identidade descartável e autorização para o relay/grupo.
