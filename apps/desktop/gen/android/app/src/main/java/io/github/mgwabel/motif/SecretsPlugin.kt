package io.github.mgwabel.motif

import android.app.Activity
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class SecretArgs {
  lateinit var key: String
  var value: String? = null
}

/**
 * Secrets encrypted with an AES key that never leaves the Android Keystore, kept in
 * private preferences. The Rust side is `apps/desktop/src/secrets.rs`.
 */
@TauriPlugin
class SecretsPlugin(private val activity: Activity) : Plugin(activity) {
  private val prefs = activity.applicationContext.getSharedPreferences("secrets", Context.MODE_PRIVATE)

  /** Resolves `{ value }`, null when there is none (or it can't be read, e.g. restored from a backup without its key). */
  @Command
  fun get(invoke: Invoke) {
    val key = invoke.parseArgs(SecretArgs::class.java).key
    val value = prefs.getString(key, null)?.let {
      try {
        decrypt(key, it)
      } catch (e: Exception) {
        null
      }
    }
    invoke.resolve(JSObject().put("value", value))
  }

  /** Stores `value` under `key`, or forgets it if `value` is null. */
  @Command
  fun set(invoke: Invoke) {
    val args = invoke.parseArgs(SecretArgs::class.java)
    try {
      val edit = prefs.edit()
      args.value?.let { edit.putString(args.key, encrypt(args.key, it)) } ?: edit.remove(args.key)
      if (edit.commit()) invoke.resolve() else invoke.reject("can't save the secret")
    } catch (e: Exception) {
      invoke.reject("can't encrypt the secret: $e")
    }
  }

  private fun encrypt(key: String, value: String): String {
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.ENCRYPT_MODE, keystoreKey())
    // Ties the ciphertext to its name, so one secret can't be swapped for another.
    cipher.updateAAD(key.toByteArray())
    return Base64.encodeToString(cipher.iv + cipher.doFinal(value.toByteArray()), Base64.NO_WRAP)
  }

  private fun decrypt(key: String, stored: String): String {
    val bytes = Base64.decode(stored, Base64.NO_WRAP)
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.DECRYPT_MODE, keystoreKey(), GCMParameterSpec(128, bytes, 0, IV_SIZE))
    cipher.updateAAD(key.toByteArray())
    return String(cipher.doFinal(bytes, IV_SIZE, bytes.size - IV_SIZE))
  }

  private fun keystoreKey(): SecretKey {
    val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
    (store.getKey(ALIAS, null) as SecretKey?)?.let { return it }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
    generator.init(
      KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .build()
    )
    return generator.generateKey()
  }

  private companion object {
    const val KEYSTORE = "AndroidKeyStore"
    const val ALIAS = "motif-secrets"
    const val TRANSFORMATION = "AES/GCM/NoPadding"
    const val IV_SIZE = 12
  }
}
