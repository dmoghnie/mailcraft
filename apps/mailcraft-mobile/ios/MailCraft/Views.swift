import SwiftUI

@main
struct MailCraftApp: App {
    @StateObject private var mail = Mail()

    var body: some Scene {
        WindowGroup {
            Screens(mail: mail)
                .tint(Color(red: 0x0E / 255, green: 0x7C / 255, blue: 0x86 / 255))
        }
    }
}

/// The screen the core says. Navigation is the core's: Back is an action, not a stack here.
struct Screens: View {
    @ObservedObject var mail: Mail

    var body: some View {
        NavigationStack {
            Group {
                if !mail.view.ready {
                    ProgressView()
                } else {
                    switch mail.view.screen {
                    case "messages": MessagesView(mail: mail)
                    case "message": MessageView(mail: mail)
                    case "compose": ComposeView(mail: mail)
                    case "addAccount": AddAccountView(mail: mail)
                    default: FoldersView(mail: mail)
                    }
                }
            }
            .toolbar {
                if mail.view.screen != "folders" {
                    ToolbarItem(placement: .topBarLeading) { Button("Back") { mail.act("back") } }
                }
            }
            .navigationBarBackButtonHidden(true)
        }
        .alert("MailCraft", isPresented: Binding(get: { mail.problem != nil || mail.view.notice != nil }, set: { if !$0 { mail.dismiss() } })) {
            Button("OK") { mail.dismiss() }
        } message: {
            Text(mail.problem ?? mail.view.notice ?? "")
        }
    }
}

private let order = ["inbox", "drafts", "sent", "archive", "junk", "trash"]

struct FoldersView: View {
    @ObservedObject var mail: Mail

    var body: some View {
        List {
            if mail.view.accounts.count > 1 {
                Button("All inboxes") { mail.act("unifiedInbox") }.bold()
            }
            ForEach(mail.view.accounts) { a in
                Section {
                    ForEach(sorted(a.folders), id: \.path) { f in
                        Button { mail.act("openFolder", ["account": a.id, "folder": f.path]) } label: {
                            HStack {
                                Text(f.name).fontWeight(f.role == "inbox" ? .semibold : .regular)
                                Spacer()
                                if let u = f.unread, u > 0 { Text("\(u)").foregroundStyle(.tint) }
                            }
                        }
                        .foregroundStyle(.primary)
                    }
                } header: {
                    HStack {
                        Text(a.email)
                        Spacer()
                        Menu("…") { Button("Remove account", role: .destructive) { mail.act("removeAccount", ["account": a.id]) } }
                    }
                } footer: {
                    if let e = a.error { Text(e) }
                }
            }
        }
        .overlay { if mail.view.accounts.isEmpty { ContentUnavailableView { Text("No account yet") } actions: { Button("Add an account") { mail.act("startAdding") } } } }
        .navigationTitle("MailCraft")
        .toolbar {
            ToolbarItem { Button("Add") { mail.act("startAdding") } }
            ToolbarItem { Button("Refresh") { mail.act("refresh") } }
        }
        .refreshable { mail.act("refresh") }
    }

    private func sorted(_ f: [Folder]) -> [Folder] {
        f.sorted { a, b in
            let (i, j) = (order.firstIndex(of: a.role) ?? order.count, order.firstIndex(of: b.role) ?? order.count)
            return i != j ? i < j : a.path.lowercased() < b.path.lowercased()
        }
    }
}

struct MessagesView: View {
    @ObservedObject var mail: Mail
    @State private var query = ""

    var body: some View {
        List {
            ForEach(mail.view.messages) { m in
                Button { mail.act("openMessage", ["key": m.key]) } label: { RowView(m: m) }
                    .foregroundStyle(.primary)
                    .swipeActions(edge: .trailing) {
                        Button("Delete", role: .destructive) { mail.act("row", ["key": m.key, "action": "delete"]) }
                        Button((m.flagged ?? false) ? "Unflag" : "Flag") { mail.act("row", ["key": m.key, "action": "flag"]) }.tint(.orange)
                    }
                    .swipeActions(edge: .leading) {
                        Button((m.unread ?? false) ? "Read" : "Unread") { mail.act("row", ["key": m.key, "action": "read"]) }.tint(.accentColor)
                    }
            }
            if mail.view.messages.count < mail.view.total {
                Button("Load more") { mail.act("loadMore") }
            }
        }
        .listStyle(.plain)
        .navigationTitle(mail.view.title)
        .searchable(text: $query, prompt: "Search mail")
        .onSubmit(of: .search) { if !query.isEmpty { mail.act("search", ["query": query]) } }
        .onChange(of: query) { _, q in if q.isEmpty && mail.view.searching != nil { mail.act("clearSearch") } }
        .refreshable { mail.act("refresh") }
        .toolbar { ToolbarItem(placement: .bottomBar) { Button("New mail") { mail.act("compose", ["kind": "new"]) } } }
    }
}

struct RowView: View {
    let m: Row

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(name(m.from ?? "")).fontWeight((m.unread ?? false) ? .bold : .regular).lineLimit(1)
                Spacer()
                if let d = m.date { Text(Date(timeIntervalSince1970: TimeInterval(d)), style: .date).font(.caption).foregroundStyle(.secondary) }
            }
            HStack {
                Text((m.subject ?? "").isEmpty ? "(no subject)" : m.subject ?? "").lineLimit(1)
                if m.attachments ?? false { Image(systemName: "paperclip").font(.caption) }
                if m.flagged ?? false { Image(systemName: "flag.fill").foregroundStyle(.red).font(.caption) }
            }
            Text(m.preview ?? "").font(.caption).foregroundStyle(.secondary).lineLimit(2)
        }
    }

    /// The name part of `Name <address>`, else the address.
    private func name(_ from: String) -> String {
        guard let i = from.firstIndex(of: "<"), i > from.startIndex else { return from.trimmingCharacters(in: CharacterSet(charactersIn: "<> ")) }
        return String(from[..<i]).trimmingCharacters(in: CharacterSet(charactersIn: "\" "))
    }
}

struct MessageView: View {
    @ObservedObject var mail: Mail

    var body: some View {
        ScrollView {
            if let m = mail.view.message {
                VStack(alignment: .leading, spacing: 6) {
                    Text((m.subject ?? "").isEmpty ? "(no subject)" : m.subject ?? "").font(.title2)
                    Text(m.from ?? "").font(.headline)
                    if let to = m.to, !to.isEmpty { Text("To: " + to).font(.caption).foregroundStyle(.secondary) }
                    if let cc = m.cc, !cc.isEmpty { Text("Cc: " + cc).font(.caption).foregroundStyle(.secondary) }
                    if let a = m.attachments, !a.isEmpty { Label(a.compactMap(\.name).joined(separator: ", "), systemImage: "paperclip").font(.caption) }
                    Divider()
                    // HTML mail comes as the engine's text: nothing remote is loaded.
                    Text(m.text ?? "").textSelection(.enabled)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding()
            } else {
                ProgressView()
            }
        }
        .toolbar {
            if let m = mail.view.message {
                ToolbarItem { Button("Flag") { mail.act("row", ["key": m.key, "action": "flag"]) } }
                ToolbarItem { Button("Delete", role: .destructive) { mail.act("row", ["key": m.key, "action": "delete"]) } }
            }
            ToolbarItemGroup(placement: .bottomBar) {
                Button("Reply") { mail.act("compose", ["kind": "reply"]) }
                Spacer()
                Button("Reply all") { mail.act("compose", ["kind": "replyAll"]) }
                Spacer()
                Button("Forward") { mail.act("compose", ["kind": "forward"]) }
            }
        }
    }
}

/// The message being written: what's typed is kept here while the same message is open, and
/// sent to the core as it changes.
struct ComposeView: View {
    @ObservedObject var mail: Mail
    @State private var id = -1
    @State private var to = ""
    @State private var cc = ""
    @State private var subject = ""
    @State private var text = ""

    var body: some View {
        Form {
            if let c = mail.view.compose {
                if let e = c.error { Text(e).foregroundStyle(.red) }
                if !c.from.isEmpty { Text("From: " + c.from).font(.caption) }
            }
            TextField("To", text: $to).keyboardType(.emailAddress).textInputAutocapitalization(.never)
                .onChange(of: to) { _, v in mail.act("edit", ["to": v]) }
            TextField("Cc", text: $cc).keyboardType(.emailAddress).textInputAutocapitalization(.never)
                .onChange(of: cc) { _, v in mail.act("edit", ["cc": v]) }
            TextField("Subject", text: $subject).onChange(of: subject) { _, v in mail.act("edit", ["subject": v]) }
            TextEditor(text: $text).frame(minHeight: 240).onChange(of: text) { _, v in mail.act("edit", ["body": v]) }
        }
        .navigationTitle(subject.isEmpty ? "New message" : subject)
        .onAppear(perform: load)
        .onChange(of: mail.view.compose?.id) { _, _ in load() }
        .toolbar {
            ToolbarItem { Button("Send") { mail.act("send") }.disabled(to.isEmpty || (mail.view.compose?.sending ?? false)) }
            ToolbarItem { Menu("…") { Button("Discard", role: .destructive) { mail.act("discard") } } }
        }
    }

    private func load() {
        guard let c = mail.view.compose, c.id != id else { return }
        (id, to, cc, subject, text) = (c.id, c.to, c.cc, c.subject, c.body)
    }
}

struct AddAccountView: View {
    @ObservedObject var mail: Mail
    @State private var email = ""
    @State private var password = ""
    @State private var name = ""
    @State private var servers = false
    @State private var imap = ""
    @State private var smtp = ""

    var body: some View {
        Form {
            TextField("Email address", text: $email).keyboardType(.emailAddress).textInputAutocapitalization(.never).autocorrectionDisabled()
            SecureField("Password", text: $password)
            TextField("Your name (optional)", text: $name)
            Toggle("Type the servers", isOn: $servers)
            if servers {
                TextField("Incoming (IMAP) host:port", text: $imap).textInputAutocapitalization(.never)
                TextField("Outgoing (SMTP) host:port", text: $smtp).textInputAutocapitalization(.never)
            }
            if mail.view.adding { ProgressView("Looking up the servers…") }
            Button("Add") {
                mail.act("addAccount", [
                    "email": email.trimmingCharacters(in: .whitespaces), "password": password,
                    "name": name.isEmpty ? nil : name,
                    "imap": servers && !imap.isEmpty ? imap : nil, "smtp": servers && !smtp.isEmpty ? smtp : nil,
                ])
            }
            .disabled(mail.view.adding || email.isEmpty || password.isEmpty)
        }
        .navigationTitle("Add account")
    }
}
