# Relatório de Milestone — M<N>: <nome>

> Preencha **todas** as seções. Se uma não se aplicar, escreva "N/A" e explique em uma linha.
> Este relatório é o corpo do PR `M<N>: <nome>`.

## 1. Resumo
<!-- 3–5 linhas: o que foi entregue, em linguagem direta. -->

## 2. Critérios de aceite

| # | Critério (copiado do MILESTONES.md) | Status | Evidência |
|---|---|---|---|
| 1 | | ✅ / ⚠️ parcial / ❌ / 🔧 requer hardware | teste `nome_do_teste`, log, captura de tela |

## 3. Definition of Done

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` — <N> testes passando
- [ ] CI verde — link:
- [ ] Sem `unwrap`/`expect` não justificados
- [ ] `USER_MANUAL.md` atualizado — seções:
- [ ] Testado com `--source=test` / `--source=file:`

## 4. O que foi feito

| Arquivo / módulo | Mudança |
|---|---|
| | |

## 5. Desvios da arquitetura ou das convenções
<!-- Qualquer coisa feita diferente do ARCHITECTURE.md / CONVENTIONS.md. Se nenhum: "Nenhum". -->

| O que diz o documento | O que foi feito | Por quê |
|---|---|---|

## 6. Dependências adicionadas

| Crate | Versão | Está na lista aprovada? | Justificativa |
|---|---|---|---|

## 7. Testes para o Henrique rodar no hardware
<!-- Passos numerados, com comandos exatos e o resultado esperado. -->

1. `comando`
   - Esperado:

## 8. Problemas conhecidos / limitações
<!-- Bugs encontrados e não corrigidos, comportamentos estranhos, TODOs deixados no código (com arquivo:linha). -->

## 9. Perguntas para a engenharia
<!-- Decisões que você não tomou por estarem fora do seu escopo. Seja específico, com opções A/B se possível. -->

## 10. Sugestão para o próximo milestone
<!-- Opcional: riscos vistos para M<N+1>. Não implementar. -->
