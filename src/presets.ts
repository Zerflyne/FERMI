import type { Questions } from "./api";

export interface Preset { name: string; hint: string; state: string; questions: Questions }

const j = (o: unknown) => JSON.stringify(o, null, 1);

export const PRESETS: Preset[] = [
  {
    name: "Ticket di assistenza", hint: "smistamento automatico",
    state: j({ subject: "Addebitato due volte!!", from: "giulia.r@example.com",
               body: "Buongiorno, a marzo mi avete addebitato l'abbonamento due volte. Ho già scritto la settimana scorsa e nessuno mi ha risposto. Rivoglio i soldi entro venerdì, altrimenti disdico." }),
    questions: {
      rimborso: { type: "noul", instructions: "Il cliente chiede un rimborso?" },
      reparto: { type: "choice", instructions: "Quale reparto deve gestire il ticket?",
                 criteria: { fatturazione: "Pagamenti, addebiti, fatture", tecnico: "Errori, guasti, accesso", commerciale: "Offerte, nuovi acquisti", altro: "Tutto il resto" } },
      urgenza: { type: "score", instructions: "Quanto è urgente?", criteria: ["Bassa", "Media", "Alta"] },
      gia_scritto: { type: "noul", instructions: "Il cliente aveva già contattato l'assistenza prima?" },
    },
  },
  {
    name: "Recensione hotel", hint: "analisi delle recensioni",
    state: "La nostra soggiornata in questo albergo è stata un po' deludente. La stanza era piccola e poco ariosa, il che ci ha fatto sentire un po' soffocati, soprattutto durante le notti in cui il rumore di strada si faceva sentire. Il personale, pur essere stato cortese, sembrava scarsamente interessato al nostro benessere e la colazione proposta era scarsa e di poco valore.",
    questions: {
      camera: { type: "noul", instructions: "La recensione menziona specificamente problemi di spazio nella camera?",
                criteria: { true: "Si fa esplicito riferimento alla dimensione o all'aria della stanza", false: "Nessun riferimento specifico alla dimensione o all'aria della stanza" } },
      viaggiatore: { type: "choice", instructions: "Che tipo di viaggiatore ha scritto la recensione?",
                     criteria: { lavoro: "Viaggio di lavoro", vacanza: "Vacanza, relax, turismo", altro: "Non specificato" } },
      soddisfazione: { type: "score", instructions: "Quanto è soddisfatto il cliente?",
                       criteria: ["Molto insoddisfatto", "Insoddisfatto", "Neutro", "Soddisfatto", "Entusiasta"] },
      aspetti: { type: "choice", instructions: "Qual è l'aspetto criticato di più?",
                 criteria: { camera: "Camera, letto, pulizia", rumore: "Rumore", personale: "Personale e servizio", colazione: "Colazione e cibo", prezzo: "Prezzo" } },
    },
  },
  {
    name: "Moderazione chat", hint: "messaggi da approvare",
    state: j({ platform: "community di videogiochi", channel: "#generale",
               message: "ma sei proprio un incapace, disinstalla il gioco e non farti più vedere qui" }),
    questions: {
      accettabile: { type: "noul", instructions: "Il messaggio rispetta le regole di una community civile?" },
      tono: { type: "choice", instructions: "Qual è il tono del messaggio?",
              criteria: { amichevole: "", neutro: "", sarcastico: "", offensivo: "", minaccioso: "" } },
      gravita: { type: "score", instructions: "Quanto è grave la violazione?", criteria: ["Nessuna", "Lieve", "Media", "Grave"] },
    },
  },
  {
    name: "Email sospetta", hint: "riconoscere il phishing",
    state: j({ from: "sicurezza@poste-verifica-account.com", subject: "Il tuo conto è stato sospeso",
               body: "Gentile cliente, abbiamo rilevato un accesso anomalo. Il tuo conto verrà chiuso entro 24 ore. Clicca qui per verificare i tuoi dati: http://poste-verifica-account.com/login" }),
    questions: {
      phishing: { type: "noul", instructions: "È un tentativo di phishing?" },
      leva: { type: "choice", instructions: "Su cosa fa leva il messaggio?",
              criteria: { paura: "Minaccia di blocco o chiusura", premio: "Vincite, regali, rimborsi", curiosita: "Foto, documenti, pacchi", nessuna: "Messaggio normale" } },
      rischio: { type: "score", instructions: "Quanto è pericoloso aprire il link?", criteria: ["Nessun rischio", "Basso", "Alto", "Altissimo"] },
    },
  },
  {
    name: "Log del server", hint: "allarmi su dati tecnici",
    state: ["2026-10-02 03:14:07 INFO  api     GET /health 200 3ms",
            "2026-10-02 03:14:09 WARN  db      slow query 2840ms: SELECT * FROM orders WHERE ...",
            "2026-10-02 03:14:12 ERROR db      connection pool exhausted (50/50)",
            "2026-10-02 03:14:12 ERROR api     POST /checkout 503 30001ms",
            "2026-10-02 03:14:13 ERROR api     POST /checkout 503 30002ms"].join("\n"),
    questions: {
      incidente: { type: "noul", instructions: "C'è un incidente in corso che va segnalato a qualcuno?" },
      componente: { type: "choice", instructions: "Quale componente è la causa principale?",
                    criteria: { database: "", api: "", rete: "", disco: "" } },
      gravita: { type: "score", instructions: "Quanto è grave?", criteria: ["Normale", "Da tenere d'occhio", "Degrado del servizio", "Servizio fermo"] },
    },
  },
  {
    name: "Candidatura", hint: "pre-selezione dei CV",
    state: j({ annuncio: "Sviluppatore backend Python, almeno 3 anni di esperienza, Django o FastAPI, PostgreSQL. Inglese buono.",
               cv: "Marco, 29 anni. 4 anni come sviluppatore in un'agenzia web: API REST in FastAPI, PostgreSQL e Redis, deploy su AWS. Prima 1 anno di PHP. Inglese B2." }),
    questions: {
      requisiti: { type: "score", instructions: "Quanto il CV soddisfa i requisiti dell'annuncio?", criteria: ["Per niente", "In parte", "Quasi tutti", "Tutti"] },
      livello: { type: "choice", instructions: "Che livello di esperienza ha il candidato?",
                 criteria: { junior: "Meno di 2 anni", mid: "Da 2 a 5 anni", senior: "Più di 5 anni" } },
      colloquio: { type: "noul", instructions: "Vale la pena chiamarlo per un colloquio?" },
    },
  },
];

/** ready-made question sets for the folder analysis */
export const QUESTION_SETS: { name: string; questions: Questions }[] = [
  {
    name: "Smistamento ticket",
    questions: {
      urgenza: { type: "score", instructions: "Quanto è urgente questo ticket?",
                 criteria: ["Bassa: può aspettare", "Media: da gestire a breve", "Alta: blocca il lavoro o costa denaro"] },
      reparto: { type: "choice", instructions: "Quale reparto deve gestire questo ticket?",
                 criteria: { tecnico: "Errori, guasti, configurazioni, integrazioni", fatturazione: "Pagamenti, fatture, addebiti, abbonamenti",
                             prodotto: "Domande su come funziona un prodotto o servizio", resi: "Resi, cambi, prodotti danneggiati", altro: "Tutto il resto" } },
      tipo: { type: "choice", instructions: "Che tipo di ticket è?",
              criteria: { incidente: "Qualcosa ha smesso di funzionare all'improvviso", richiesta: "Chiede informazioni o un servizio",
                          problema: "Un difetto che si ripete o di cui va trovata la causa", modifica: "Chiede di cambiare una configurazione o un sistema" } },
    },
  },
  {
    name: "Recensioni",
    questions: {
      soddisfazione: { type: "score", instructions: "Quanto è soddisfatto il cliente?", criteria: ["Molto insoddisfatto", "Insoddisfatto", "Neutro", "Soddisfatto", "Entusiasta"] },
      consiglia: { type: "noul", instructions: "Il cliente consiglierebbe il prodotto o il servizio?" },
      tema: { type: "choice", instructions: "Di cosa parla soprattutto la recensione?",
              criteria: { qualita: "Qualità del prodotto", prezzo: "Prezzo", spedizione: "Spedizione e consegna", assistenza: "Assistenza clienti", altro: "Altro" } },
    },
  },
  {
    name: "Moderazione",
    questions: {
      accettabile: { type: "noul", instructions: "Il messaggio rispetta le regole di una community civile?" },
      tono: { type: "choice", instructions: "Qual è il tono del messaggio?", criteria: { amichevole: "", neutro: "", sarcastico: "", offensivo: "", minaccioso: "" } },
      gravita: { type: "score", instructions: "Quanto è grave la violazione?", criteria: ["Nessuna", "Lieve", "Media", "Grave"] },
    },
  },
];
