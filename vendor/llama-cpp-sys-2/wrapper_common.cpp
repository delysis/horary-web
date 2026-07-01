#include "wrapper_common.h"

#include <cstdlib>
#include <cstring>
#include <exception>
#include <algorithm>
#include <string>
#include <stdint.h>
#include <vector>

#include "llama.cpp/common/common.h"
#include "llama.cpp/common/fit.h"
#include "llama.cpp/common/json-schema-to-grammar.h"
#include "llama.cpp/common/speculative.h"
#include "llama.cpp/include/llama.h"
#include "wrapper_utils.h"

#include <nlohmann/json.hpp>

extern "C" llama_rs_status llama_rs_json_schema_to_grammar(
    const char * schema_json,
    bool force_gbnf,
    char ** out_grammar) {
    if (!schema_json || !out_grammar) {
        return LLAMA_RS_STATUS_INVALID_ARGUMENT;
    }

    *out_grammar = nullptr;
    try {
        const auto schema = nlohmann::ordered_json::parse(schema_json);
        const auto grammar = json_schema_to_grammar(schema, force_gbnf);
        *out_grammar = llama_rs_dup_string(grammar);
        return *out_grammar ? LLAMA_RS_STATUS_OK : LLAMA_RS_STATUS_ALLOCATION_FAILED;
    } catch (const std::exception &) {
        return LLAMA_RS_STATUS_EXCEPTION;
    }
}

extern "C" void llama_rs_string_free(char * ptr) {
    if (ptr) {
        std::free(ptr);
    }
}

extern "C" struct llama_sampler * llama_rs_sampler_init_grammar(
    const struct llama_vocab * vocab,
    const char * grammar_str,
    const char * grammar_root) {
    try {
        return llama_sampler_init_grammar(vocab, grammar_str, grammar_root);
    } catch (...) {
        return nullptr;
    }
}

extern "C" struct llama_sampler * llama_rs_sampler_init_grammar_lazy(
    const struct llama_vocab * vocab,
    const char * grammar_str,
    const char * grammar_root,
    const char ** trigger_words,
    size_t num_trigger_words,
    const llama_token * trigger_tokens,
    size_t num_trigger_tokens) {
    try {
        std::vector<std::string> trigger_patterns;
        trigger_patterns.reserve(num_trigger_words);
        for (size_t i = 0; i < num_trigger_words; ++i) {
            const char * word = trigger_words ? trigger_words[i] : nullptr;
            if (word && word[0] != '\0') {
                trigger_patterns.push_back(regex_escape(word));
            }
        }
        std::vector<const char *> trigger_patterns_c;
        trigger_patterns_c.reserve(trigger_patterns.size());
        for (const auto & pattern : trigger_patterns) {
            trigger_patterns_c.push_back(pattern.c_str());
        }
        return llama_sampler_init_grammar_lazy_patterns(
            vocab,
            grammar_str,
            grammar_root,
            trigger_patterns_c.data(),
            trigger_patterns_c.size(),
            trigger_tokens,
            num_trigger_tokens);
    } catch (...) {
        return nullptr;
    }
}

extern "C" struct llama_sampler * llama_rs_sampler_init_grammar_lazy_patterns(
    const struct llama_vocab * vocab,
    const char * grammar_str,
    const char * grammar_root,
    const char ** trigger_patterns,
    size_t num_trigger_patterns,
    const llama_token * trigger_tokens,
    size_t num_trigger_tokens) {
    try {
        return llama_sampler_init_grammar_lazy_patterns(
            vocab,
            grammar_str,
            grammar_root,
            trigger_patterns,
            num_trigger_patterns,
            trigger_tokens,
            num_trigger_tokens);
    } catch (...) {
        return nullptr;
    }
}

extern "C" llama_rs_status llama_rs_sampler_accept(struct llama_sampler * sampler, llama_token token) {
    if (!sampler) {
        return LLAMA_RS_STATUS_INVALID_ARGUMENT;
    }
    try {
        llama_sampler_accept(sampler, token);
        return LLAMA_RS_STATUS_OK;
    } catch (const std::exception &) {
        return LLAMA_RS_STATUS_EXCEPTION;
    } catch (...) {
        return LLAMA_RS_STATUS_EXCEPTION;
    }
}

struct llama_rs_speculative {
    common_speculative * inner = nullptr;
};

static llama_tokens llama_rs_tokens_from_raw(const llama_token * tokens, size_t n_tokens) {
    llama_tokens result;
    if (tokens == nullptr || n_tokens == 0) {
        return result;
    }
    result.assign(tokens, tokens + n_tokens);
    return result;
}

extern "C" llama_rs_status llama_rs_speculative_mtp_init(
    struct llama_context * ctx_tgt,
    struct llama_context * ctx_dft,
    struct llama_rs_speculative_mtp_params params,
    uint32_t n_seq,
    struct llama_rs_speculative ** out_spec) {
    if (!ctx_tgt || !ctx_dft || !out_spec || n_seq == 0) {
        return LLAMA_RS_STATUS_INVALID_ARGUMENT;
    }

    *out_spec = nullptr;
    try {
        common_params_speculative spec_params;
        spec_params.types = { COMMON_SPECULATIVE_TYPE_DRAFT_MTP };
        spec_params.draft.ctx_tgt = ctx_tgt;
        spec_params.draft.ctx_dft = ctx_dft;
        spec_params.draft.n_max = std::max(1, params.n_max);
        spec_params.draft.n_min = std::max(0, params.n_min);
        spec_params.draft.p_min = std::max(0.0f, params.p_min);
        spec_params.draft.backend_sampling = params.backend_sampling;

        common_speculative * inner = common_speculative_init(spec_params, n_seq);
        if (!inner) {
            return LLAMA_RS_STATUS_INVALID_ARGUMENT;
        }

        llama_rs_speculative * wrapper = new llama_rs_speculative { inner };
        *out_spec = wrapper;
        return LLAMA_RS_STATUS_OK;
    } catch (const std::bad_alloc &) {
        return LLAMA_RS_STATUS_ALLOCATION_FAILED;
    } catch (const std::exception &) {
        return LLAMA_RS_STATUS_EXCEPTION;
    } catch (...) {
        return LLAMA_RS_STATUS_EXCEPTION;
    }
}

extern "C" void llama_rs_speculative_free(struct llama_rs_speculative * spec) {
    if (!spec) {
        return;
    }
    common_speculative_free(spec->inner);
    spec->inner = nullptr;
    delete spec;
}

extern "C" bool llama_rs_speculative_need_embd_nextn(struct llama_rs_speculative * spec) {
    return spec && spec->inner && common_speculative_need_embd_nextn(spec->inner);
}

extern "C" llama_rs_status llama_rs_speculative_begin(
    struct llama_rs_speculative * spec,
    llama_seq_id seq_id,
    const llama_token * prompt,
    size_t n_prompt) {
    if (!spec || !spec->inner || (n_prompt > 0 && !prompt)) {
        return LLAMA_RS_STATUS_INVALID_ARGUMENT;
    }

    try {
        llama_tokens prompt_vec = llama_rs_tokens_from_raw(prompt, n_prompt);
        common_speculative_begin(spec->inner, seq_id, prompt_vec);
        return LLAMA_RS_STATUS_OK;
    } catch (const std::bad_alloc &) {
        return LLAMA_RS_STATUS_ALLOCATION_FAILED;
    } catch (const std::exception &) {
        return LLAMA_RS_STATUS_EXCEPTION;
    } catch (...) {
        return LLAMA_RS_STATUS_EXCEPTION;
    }
}

extern "C" llama_rs_status llama_rs_speculative_process(
    struct llama_rs_speculative * spec,
    const struct llama_batch * batch) {
    if (!spec || !spec->inner || !batch) {
        return LLAMA_RS_STATUS_INVALID_ARGUMENT;
    }

    try {
        return common_speculative_process(spec->inner, *batch)
            ? LLAMA_RS_STATUS_OK
            : LLAMA_RS_STATUS_EXCEPTION;
    } catch (const std::bad_alloc &) {
        return LLAMA_RS_STATUS_ALLOCATION_FAILED;
    } catch (const std::exception &) {
        return LLAMA_RS_STATUS_EXCEPTION;
    } catch (...) {
        return LLAMA_RS_STATUS_EXCEPTION;
    }
}

extern "C" llama_rs_status llama_rs_speculative_draft(
    struct llama_rs_speculative * spec,
    llama_seq_id seq_id,
    llama_pos n_past,
    llama_token id_last,
    const llama_token * prompt,
    size_t n_prompt,
    int32_t n_max_override,
    llama_token * out_tokens,
    size_t out_capacity,
    size_t * out_count) {
    if (!spec || !spec->inner || !out_count || (n_prompt > 0 && !prompt) ||
        (out_capacity > 0 && !out_tokens)) {
        return LLAMA_RS_STATUS_INVALID_ARGUMENT;
    }

    *out_count = 0;
    if (out_capacity == 0) {
        return LLAMA_RS_STATUS_OK;
    }

    try {
        llama_tokens prompt_vec = llama_rs_tokens_from_raw(prompt, n_prompt);
        llama_tokens result;
        result.reserve(out_capacity);

        auto & draft = common_speculative_get_draft_params(spec->inner, seq_id);
        draft.drafting = true;
        draft.n_max = n_max_override > 0
            ? std::min<int32_t>(n_max_override, static_cast<int32_t>(out_capacity))
            : static_cast<int32_t>(out_capacity);
        draft.n_past = n_past;
        draft.id_last = id_last;
        draft.prompt = &prompt_vec;
        draft.result = &result;

        common_speculative_draft(spec->inner);

        const size_t n_copy = std::min(result.size(), out_capacity);
        if (n_copy > 0) {
            std::memcpy(out_tokens, result.data(), n_copy * sizeof(llama_token));
        }
        *out_count = n_copy;

        draft.drafting = false;
        draft.n_max = -1;
        draft.prompt = nullptr;
        draft.result = nullptr;
        return LLAMA_RS_STATUS_OK;
    } catch (const std::bad_alloc &) {
        return LLAMA_RS_STATUS_ALLOCATION_FAILED;
    } catch (const std::exception &) {
        return LLAMA_RS_STATUS_EXCEPTION;
    } catch (...) {
        return LLAMA_RS_STATUS_EXCEPTION;
    }
}

extern "C" llama_rs_status llama_rs_speculative_accept(
    struct llama_rs_speculative * spec,
    llama_seq_id seq_id,
    uint16_t n_accepted) {
    if (!spec || !spec->inner) {
        return LLAMA_RS_STATUS_INVALID_ARGUMENT;
    }

    try {
        common_speculative_accept(spec->inner, seq_id, n_accepted);
        return LLAMA_RS_STATUS_OK;
    } catch (const std::exception &) {
        return LLAMA_RS_STATUS_EXCEPTION;
    } catch (...) {
        return LLAMA_RS_STATUS_EXCEPTION;
    }
}

extern "C" void llama_rs_speculative_print_stats(const struct llama_rs_speculative * spec) {
    if (!spec || !spec->inner) {
        return;
    }
    common_speculative_print_stats(spec->inner);
}

// Thin pass-through to llama.cpp's common_fit_params (a C++ symbol in libcommon).
// Returns common_params_fit_status as an int: 0 = success, 1 = failure, 2 = error.
extern "C" int llama_rs_fit_params(
    const char * path_model,
    struct llama_model_params * mparams,
    struct llama_context_params * cparams,
    float * tensor_split,
    struct llama_model_tensor_buft_override * tensor_buft_overrides,
    size_t * margins,
    uint32_t n_ctx_min,
    enum ggml_log_level log_level) {
    return static_cast<int>(common_fit_params(
        path_model,
        mparams,
        cparams,
        tensor_split,
        tensor_buft_overrides,
        margins,
        n_ctx_min,
        log_level));
}

extern "C" void llama_rs_memory_breakdown_print(const struct llama_context * ctx) {
    common_memory_breakdown_print(ctx);
}
