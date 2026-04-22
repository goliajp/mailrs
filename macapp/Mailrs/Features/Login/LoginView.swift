import SwiftUI

struct LoginView: View {
    @State var model: LoginModel
    @FocusState private var focusedField: Field?

    private enum Field { case email, password, totp }

    var body: some View {
        ZStack {
            #if os(iOS)
            Color(uiColor: .systemBackground).ignoresSafeArea()
            #endif

            ScrollView {
                VStack(spacing: 24) {
                    header

                    switch model.stage {
                    case .credentials:
                        credentialsForm
                    case .totp:
                        totpForm
                    }

                    if let err = model.errorMessage {
                        Text(err)
                            .font(.callout)
                            .foregroundStyle(.red)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }

                    submitButton

                    if model.stage == .totp {
                        Button("返回重新输入密码") {
                            model.backToCredentials()
                        }
                        .buttonStyle(.plain)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    }
                }
                .padding(32)
                .frame(maxWidth: 420)
                .frame(maxWidth: .infinity)
            }
        }
        .onAppear {
            focusedField = model.email.isEmpty ? .email : .password
        }
    }

    private var header: some View {
        VStack(spacing: 8) {
            Image(systemName: "envelope.fill")
                .font(.system(size: 48))
                .foregroundStyle(.tint)
            Text("Mailrs")
                .font(.largeTitle.bold())
            Text("登录你的邮箱账户")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .padding(.top, 24)
    }

    private var credentialsForm: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("邮箱").font(.caption).foregroundStyle(.secondary)
            TextField("you@example.com", text: $model.email)
                .textFieldStyle(.roundedBorder)
                .focused($focusedField, equals: .email)
                #if os(iOS)
                .textContentType(.emailAddress)
                .keyboardType(.emailAddress)
                .autocapitalization(.none)
                .autocorrectionDisabled(true)
                #endif
                .submitLabel(.next)
                .onSubmit { focusedField = .password }

            Text("密码").font(.caption).foregroundStyle(.secondary)
                .padding(.top, 8)
            SecureField("密码", text: $model.password)
                .textFieldStyle(.roundedBorder)
                .focused($focusedField, equals: .password)
                .submitLabel(.go)
                .onSubmit { Task { await model.submit() } }
        }
    }

    private var totpForm: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("两步验证").font(.headline)
            Text("请输入验证器应用中的 6 位数字")
                .font(.caption)
                .foregroundStyle(.secondary)
            TextField("123456", text: $model.totpCode)
                .textFieldStyle(.roundedBorder)
                .focused($focusedField, equals: .totp)
                #if os(iOS)
                .keyboardType(.numberPad)
                .textContentType(.oneTimeCode)
                #endif
                .submitLabel(.go)
                .onSubmit { Task { await model.submit() } }
                .onAppear { focusedField = .totp }
        }
    }

    private var submitButton: some View {
        Button {
            Task { await model.submit() }
        } label: {
            HStack {
                if model.isSubmitting {
                    ProgressView().controlSize(.small)
                }
                Text(model.stage == .totp ? "验证" : "登录")
                    .fontWeight(.semibold)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 6)
        }
        .buttonStyle(.borderedProminent)
        .controlSize(.large)
        .disabled(!model.canSubmit)
    }
}
